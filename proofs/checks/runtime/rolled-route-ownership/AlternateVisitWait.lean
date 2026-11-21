import Hibana.SourceOccurrenceTrace
import Hibana.PublicCandidateAdmission

namespace Hibana.RegionalHistory
open EndpointDelegation

/-- Index into this unfolding, then recover the original source position. A
repeated position can stand for several distinct physical acceptances. -/
def occurrenceOrigin? (plan : SourceOccurrenceCut source) (occurrence : Nat) : Option Nat :=
  ((plan.unfolding.occurrences 0)[occurrence]?).map Prod.fst

/-- The source correspondence explains every carrier acceptance, in order,
with the key and the original position measured in that same physical word. -/
theorem keyed_acceptance_origins (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    calls.map (fun p => (p.1, occurrenceOrigin? plan p.2)) =
      (word.filterMap DistributedCallBoundary.acceptance?).map (fun entry =>
        (entry.callKey, some entry.event)) := by
  induction trace with
  | nil => rfl
  | accept prior fresh origin matched ready ih =>
    simp [List.filterMap_append, DistributedCallBoundary.acceptance?, ih, occurrenceOrigin?, origin] at *
  | quiet prior ih | retireView prior ih | close prior ih | observeClosure prior ih
  | transportFault prior ih | abort prior ih | publish prior found matched ready ih
  | receive prior found matched ready ih | localCommit prior matched ready ih =>
    simpa [List.filterMap_append, DistributedCallBoundary.acceptance?] using ih

private def lookupOrigin (entries : List ((TransportChannel × Nat) × Option Nat))
    (key : TransportChannel × Nat) : Option Nat :=
  (entries.find? (fun p => decide (p.1 = key))).bind Prod.snd

private theorem lookup_mapped_calls (plan : SourceOccurrenceCut source) (calls : OccurrenceCalls)
    (key : TransportChannel × Nat) :
    lookupOrigin (calls.map (fun p => (p.1, occurrenceOrigin? plan p.2))) key =
      (occurrenceCall? calls key).bind (occurrenceOrigin? plan) := by
  unfold lookupOrigin occurrenceCall?
  rw [List.find?_map]
  change ((calls.find? (fun p => decide (p.1 = key))).map _).bind _ =
    ((calls.find? (fun p => decide (p.1 = key))).map _).bind _
  cases calls.find? (fun p => decide (p.1 = key)) <;> rfl

/-- A call resolved in one compatible unfolding has an occurrence in every
other replay of the physical prefix, with the same static origin. No equality
of the two occurrence indices, route assignments, or unobserved futures is
needed. Both histories use the same actual carrier key. -/
theorem accepted_call_survives_alternative_unfolding
    (first : SourceOccurrenceTrace images choreo firstPlan word firstCalls firstPoints)
    (second : SourceOccurrenceTrace images choreo secondPlan word secondCalls secondPoints)
    (resolved : occurrenceCall? firstCalls key = some occurrence) :
    ∃ other origin, occurrenceCall? secondCalls key = some other ∧
      occurrenceOrigin? firstPlan occurrence = some origin ∧
      occurrenceOrigin? secondPlan other = some origin := by
  have same := congrArg (fun entries => lookupOrigin entries key)
    ((keyed_acceptance_origins first).trans (keyed_acceptance_origins second).symm)
  dsimp only at same
  rw [lookup_mapped_calls, lookup_mapped_calls, resolved, Option.bind_some] at same
  obtain ⟨_, _, entry, _, _, origin, _⟩ :=
    a_resolved_occurrence_call_has_its_same_word_acceptance first resolved
  change occurrenceOrigin? firstPlan occurrence = some entry.event at origin
  cases otherAt : occurrenceCall? secondCalls key with
  | none => simp [otherAt, origin] at same
  | some other => exact ⟨other, entry.event, rfl, origin, by simpa [otherAt, origin] using same.symm⟩

def pointOrigin? (plan : SourceOccurrenceCut source) : SourcePoint → Option SourcePoint
  | .accepted occurrence => (occurrenceOrigin? plan occurrence).map SourcePoint.accepted
  | .committed action => (occurrenceOrigin? plan action.event).map fun origin =>
      .committed ⟨origin, action.side⟩

private def rowOrigin? (image : RustDescriptorImage) (choreo : Choreo)
    (path row : Nat) : Option SourcePoint :=
  (sourceOperationIdentity? image choreo path row).map fun (origin, _, side) =>
    .committed ⟨origin, side⟩

/-- Production records carry a row and the descriptor used to interpret it.
Stuttering boundaries have no source point. A failed row decode occupies its
chronological place as `none` so that it cannot silently erase a commit. -/
def boundaryOrigins (images : Nat → RustDescriptorImage) (choreo : Choreo) :
    DistributedCallBoundary → List (Option SourcePoint)
  | .accept entry => [some (.accepted entry.event)]
  | .publish _ actor record | .receive _ actor record | .localCommit actor record =>
      [rowOrigin? (images actor) choreo record.step.pathIndex record.step.event]
  | _ => []

private theorem matched_commit_origin (plan : SourceOccurrenceCut source)
    (matched : plan.matchesAction image choreo actor path row occurrence side = true) :
    pointOrigin? plan (.committed ⟨occurrence, side⟩) = rowOrigin? image choreo path row := by
  unfold SourceOccurrenceCut.matchesAction at matched
  unfold pointOrigin? occurrenceOrigin? rowOrigin?
  split at matched <;> simp_all <;> grind

/-- Replaying the prefix on any selected expansion recovers the measured
chronological source positions and precise sides, including both physical
acceptance and subsequent sender publication. -/
theorem chronological_point_origins (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    points.map (pointOrigin? plan) = word.flatMap (boundaryOrigins images choreo) := by
  induction trace with
  | nil => rfl
  | accept prior fresh origin matched ready ih =>
    simp [List.flatMap_append, boundaryOrigins, pointOrigin?, occurrenceOrigin?, ih, origin]
  | publish prior found matched ready ih | receive prior found matched ready ih
  | localCommit prior matched ready ih =>
    simp [List.flatMap_append, boundaryOrigins, ih, matched_commit_origin _ matched]
  | quiet prior ih | retireView prior ih | close prior ih | observeClosure prior ih
  | transportFault prior ih | abort prior ih =>
    simpa [List.flatMap_append, boundaryOrigins] using ih

/-- A predicate on all source positions and sides in the observed prefix can
be transported to a different unfolding. This includes predicates depending
on the order and multiplicity of past sends, publications, and receipts. -/
theorem observed_origin_predicate_iff
    (first : SourceOccurrenceTrace images choreo firstPlan word firstCalls firstPoints)
    (second : SourceOccurrenceTrace images choreo secondPlan word secondCalls secondPoints)
    (pred : List (Option SourcePoint) → Prop) :
    pred (firstPoints.map (pointOrigin? firstPlan)) ↔
      pred (secondPoints.map (pointOrigin? secondPlan)) := by
  rw [chronological_point_origins first, chronological_point_origins second]

#print axioms keyed_acceptance_origins
#print axioms accepted_call_survives_alternative_unfolding
#print axioms chronological_point_origins
#print axioms observed_origin_predicate_iff

end Hibana.RegionalHistory

namespace Hibana.AlternateVisitWait
open EndpointDelegation

/-- The half-open bounds are original source positions, shared by all visits
of a roll. A whole view can also be held as the payload of a delegated send. -/
structure ViewLease where
  key : EndpointKey
  bounds : Option (Nat × Nat)

def ViewLease.contains (lease : ViewLease) (plan : SourceOccurrenceCut source)
    (event : Nat) : Bool :=
  match lease.bounds with
  | none => true
  | some (start, stop) =>
    (RegionalHistory.occurrenceOrigin? plan event).any fun origin =>
      decide (start ≤ origin ∧ origin < stop)

/-- A fragment cannot gain or lose an already accepted call when an
unresolved branch is explained by a different unfolding of the same word. -/
theorem accepted_call_preserves_fragment_membership (lease : ViewLease)
    (first : SourceOccurrenceTrace images choreo firstPlan word firstCalls firstPoints)
    (second : SourceOccurrenceTrace images choreo secondPlan word secondCalls secondPoints)
    (found : occurrenceCall? firstCalls key = some occurrence) :
    ∃ other, occurrenceCall? secondCalls key = some other ∧
      lease.contains firstPlan occurrence = lease.contains secondPlan other := by
  obtain ⟨other, origin, resolved, left, right⟩ :=
    RegionalHistory.accepted_call_survives_alternative_unfolding first second found
  refine ⟨other, resolved, ?_⟩
  unfold ViewLease.contains
  cases lease.bounds with
  | none => rfl
  | some bounds => simp [left, right]

/-- Parallel arm boundaries in the original global. A repeated visit uses the
same boundaries, while its occurrence is recovered from the unfolding. -/
private def parallelForks : Source → Nat → List Scope
  | .send _, _ => []
  | .seq l r, base | .route _ l r, base =>
    parallelForks l base ++ parallelForks r (base + l.eventCount)
  | .par l r, base =>
    ⟨base, base + l.eventCount, base + l.eventCount + r.eventCount⟩ ::
      (parallelForks l base ++ parallelForks r (base + l.eventCount))
  | .roll body, base => parallelForks body base

private def siblingCarrier (source : Source) (lo hi event : Nat) : Bool :=
  decide (lo < hi) && (parallelForks source 0).any fun fork =>
    decide ((fork.start ≤ lo ∧ hi ≤ fork.split ∧ fork.split ≤ event ∧ event < fork.stop) ∨
      (fork.start ≤ event ∧ event < fork.split ∧ fork.split ≤ lo ∧ hi ≤ fork.stop))

private theorem fork_bounds {source : Source} {base : Nat} {fork : Scope}
    (member : fork ∈ parallelForks source base) :
    base ≤ fork.start ∧ fork.start ≤ fork.split ∧ fork.split ≤ fork.stop ∧
      fork.stop ≤ base + source.eventCount := by
  induction source generalizing base with
  | send => simp [parallelForks] at member
  | roll body ih => exact ih member
  | seq l r ihL ihR | route _ l r ihL ihR =>
    rcases List.mem_append.mp member with left | right
    · have := ihL left; simp only [Source.eventCount]; omega
    · have := ihR right; simp only [Source.eventCount]; omega
  | par l r ihL ihR =>
    rcases List.mem_cons.mp member with rfl | children
    · simp [Source.eventCount, Nat.add_assoc]
    · rcases List.mem_append.mp children with left | right
      · have := ihL left; simp only [Source.eventCount]; omega
      · have := ihR right; simp only [Source.eventCount]; omega

/-- A same-role prefix or continuation across a sequence has no sibling
handle while the parent is exclusively borrowed, including on another visit. -/
theorem sequential_prefix_or_tail_has_no_sibling_handle (left right : Source) (lo hi event : Nat)
    (separate : (hi ≤ left.eventCount ∧ left.eventCount ≤ event) ∨
      (event < left.eventCount ∧ left.eventCount ≤ lo)) :
    siblingCarrier (.seq left right) lo hi event = false := by
  apply Bool.eq_false_iff.mpr
  intro allowed
  obtain ⟨range, choices⟩ := Bool.and_eq_true_iff.mp allowed
  obtain ⟨fork, member, arms⟩ := List.any_eq_true.mp choices
  simp only [decide_eq_true_eq] at range arms
  rcases List.mem_append.mp member with first | second
  · have := fork_bounds first; omega
  · have := fork_bounds second; omega

private def ViewLease.blocksCarrier (lease : ViewLease) (plan : SourceOccurrenceCut source)
    (event : Nat) : Bool :=
  match lease.bounds with
  | none => true
  | some (lo, hi) =>
    !(RegionalHistory.occurrenceOrigin? plan event).any fun origin => siblingCarrier source lo hi origin

private def ViewLease.required (lease : ViewLease) (plan : SourceOccurrenceCut source)
    (binding : Binding) (id : ActionId) : Bool :=
  match id.side with
  | .receive => binding.message.receiver == lease.key && lease.blocksCarrier plan id.event
  | .send | .local =>
    (binding.message.sender == lease.key && lease.blocksCarrier plan id.event) ||
      binding.message.payload == .endpoint lease.key

/-- Other carriers remain available through genuine sibling handles.
Transferring the endpoint as a payload requires the whole endpoint. -/
theorem whole_lease_requires_exactly_the_source_endpoint (plan : SourceOccurrenceCut source)
    (key : EndpointKey) (binding : Binding) (id : ActionId) :
    ViewLease.required ⟨key, none⟩ plan binding id =
      sourceActionRequiresEndpoint key binding id.side := by
  cases id with | mk event side => cases side <;> simp [ViewLease.required, ViewLease.blocksCarrier,
    sourceActionRequiresEndpoint]

/-- An observation belongs to a send (including transport acceptance), receive
or local call on a particular view. Sending an owned endpoint also holds its
payload view. Other actors can continue while this caller is suspended. -/
def independent (plan : SourceOccurrenceCut source) (actor : Nat) (held : List ViewLease)
    (point : SourcePoint) : Bool := Id.run do
  let id : ActionId := match point with
    | .accepted event => ⟨event, .send⟩
    | .committed action => action
  let some (_, binding) := plan.path.bindings.find? (fun entry => entry.1 == id.event)
    | return false
  let some performer := occurrenceActor? plan point | return false
  return performer != actor || !held.any (fun lease => lease.required plan binding id)

def enabled (plan : SourceOccurrenceCut source) (history : List SourcePoint)
    (point : SourcePoint) : Bool :=
  let cut := occurrenceCut plan history
  cut.ready point && cut.resumptionReady point

/-- A source continuation ending at the same occurrence the caller is waiting
for. Each intervening action can run while the caller retains its views. -/
inductive CanResume (plan : SourceOccurrenceCut source) (actor : Nat) (held : List ViewLease)
    (target : SourcePoint) : List SourcePoint → Prop where
  | now {history} : enabled plan history target = true → CanResume plan actor held target history
  | later {history point} : enabled plan history point = true →
      independent plan actor held point = true →
      CanResume plan actor held target (point :: history) →
      CanResume plan actor held target history

/-- Finite reference decision for one selected unfolding. The depth is the
number of remaining source points. No time limit or chosen scheduling order
may turn a missing witness into a refusal. Only small fixtures run the search. -/
private def search (plan : SourceOccurrenceCut source) (actor : Nat) (held : List ViewLease)
    (target : SourcePoint) (history remaining : List SourcePoint) : Nat → Bool
  | 0 => enabled plan history target
  | n + 1 => enabled plan history target || remaining.any fun point =>
      enabled plan history point && independent plan actor held point &&
        search plan actor held target (point :: history) (remaining.erase point) n

private def covers (plan : SourceOccurrenceCut source) (history remaining : List SourcePoint) : Prop :=
  ∀ p ∈ sourceVisitPoints plan.path.bindings, p ∉ history → p ∈ remaining

private theorem unobserved_point {plan : SourceOccurrenceCut source}
    (h : enabled plan history p = true) :
    p ∈ sourceVisitPoints plan.path.bindings ∧ p ∉ history := by
  have h₁ := (Bool.and_eq_true_iff.mp h).1
  have h₂ := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp h₁).1)
  exact ⟨List.contains_iff_mem.mp h₂.1, by simpa [occurrenceCut] using h₂.2⟩

private theorem erase_covers {plan : SourceOccurrenceCut source}
    (h : covers plan history remaining) : covers plan (point :: history) (remaining.erase point) := by
  intro p member fresh
  have diff : p ≠ point := fun eq => fresh (List.mem_cons.mpr (.inl eq))
  exact (List.mem_erase_of_ne diff).mpr (h p member (fun seen => fresh (List.mem_cons_of_mem _ seen)))

private theorem search_exact (plan : SourceOccurrenceCut source) (actor : Nat) (held : List ViewLease)
    (target : SourcePoint) (history remaining : List SourcePoint) (fuel : Nat)
    (cover : covers plan history remaining) (bound : remaining.length ≤ fuel) :
    search plan actor held target history remaining fuel = true ↔ CanResume plan actor held target history := by
  induction fuel generalizing history remaining with
  | zero =>
    rw [search]
    constructor
    · exact CanResume.now
    · intro path
      cases path with
      | now ready => exact ready
      | later ready _ _ =>
        obtain ⟨member, fresh⟩ := unobserved_point ready
        have present := cover _ member fresh
        have positive := List.length_pos_of_mem present
        omega
  | succ n ih =>
    rw [search, Bool.or_eq_true, List.any_eq_true]
    constructor
    · rintro (ready | ⟨point, present, checked⟩)
      · exact .now ready
      · obtain ⟨guards, rest⟩ := Bool.and_eq_true_iff.mp checked
        obtain ⟨ready, free⟩ := Bool.and_eq_true_iff.mp guards
        have shorter := List.length_erase_of_mem present
        exact .later ready free ((ih _ _ (erase_covers cover) (by omega)).mp rest)
    · intro path
      cases path with
      | now ready => exact .inl ready
      | @later _ point ready free rest =>
        obtain ⟨member, fresh⟩ := unobserved_point ready
        have present := cover _ member fresh
        have shorter := List.length_erase_of_mem present
        exact .inr ⟨point, present, Bool.and_eq_true_iff.mpr
          ⟨Bool.and_eq_true_iff.mpr ⟨ready, free⟩, (ih _ _ (erase_covers cover) (by omega)).mpr rest⟩⟩

inductive PollVerdict where | ready | pending | unavailable deriving DecidableEq, Repr

def verdict (plan : SourceOccurrenceCut source) (actor : Nat) (held : List ViewLease)
    (target : SourcePoint) (history : List SourcePoint) : PollVerdict :=
  if enabled plan history target then .ready else
    let remaining := (sourceVisitPoints plan.path.bindings).filter (fun p => !history.contains p)
    if search plan actor held target history remaining remaining.length then .pending else .unavailable

/-- Exact for every finite selected source unfolding, every held set and every
cut. Pending means there is a legal extension without an action on a held view.
No progress or future scheduler fairness is claimed merely by waiting. -/
theorem verdict_exact (plan : SourceOccurrenceCut source) (actor : Nat) (held : List ViewLease)
    (target : SourcePoint) (history : List SourcePoint) :
    (verdict plan actor held target history = .ready ↔ enabled plan history target = true) ∧
    (verdict plan actor held target history = .pending ↔ enabled plan history target = false ∧
      CanResume plan actor held target history) ∧
    (verdict plan actor held target history = .unavailable ↔ ¬ CanResume plan actor held target history) := by
  let rem := (sourceVisitPoints plan.path.bindings).filter (fun p => !history.contains p)
  have c : covers plan history rem := by
    intro p member fresh
    exact List.mem_filter.mpr ⟨member, by simpa using fresh⟩
  have exactSearch := search_exact plan actor held target history rem rem.length c (Nat.le_refl _)
  cases r : enabled plan history target with
  | true =>
    have can : CanResume plan actor held target history := .now r
    simp [verdict, r, can]
  | false =>
    cases s : search plan actor held target history rem rem.length with
    | true =>
      have can := exactSearch.mp s
      have v : verdict plan actor held target history = .pending := by
        unfold verdict
        simp only [r, Bool.false_eq_true, if_false]
        change (if search plan actor held target history rem rem.length then _ else _) = _
        rw [s]
        rfl
      simp [v, can]
    | false =>
      have no : ¬ CanResume plan actor held target history := fun path => by
        have bad := exactSearch.mpr path
        simp [s] at bad
      have v : verdict plan actor held target history = .unavailable := by
        unfold verdict
        simp only [r, Bool.false_eq_true, if_false]
        change (if search plan actor held target history rem rem.length then _ else _) = _
        rw [s]
        rfl
      simp [v, no]

/-- Fresh, non-local operations subject to recoverable admission. Sending an owned endpoint also occupies its payload and has a different
failure contract. Waiting on transport or publishing an accepted send is a
later phase. -/
inductive WireCall where
  | send (schema : Nat)
  | receive (payload : Payload)

def publicCallCandidates (plan : SourceOccurrenceCut source) (actor : Nat)
    (lease : ViewLease) (label : Nat) (call : WireCall) (history : List SourcePoint) :
    List SourcePoint :=
  let (receive, payload) := match call with
    | .send schema => (false, Payload.wire schema)
    | .receive payload => (true, payload)
  plan.path.bindings.filterMap fun (event, binding) =>
    let entry := if receive then SourcePoint.committed ⟨event, .receive⟩ else .accepted event
    if decide (publicMessageContract binding.message receive = (label, payload, lease.key)) &&
        lease.contains plan event && occurrenceActor? plan entry == some actor && !history.contains entry
    then some entry else none

/-- Membership is characterized directly by the source bindings. In particular
an unobserved send to any peer with the requested schema is considered if the
calling actor performs it on this view. Receives use the actual receiver. -/
theorem public_candidates_exact (plan : SourceOccurrenceCut source) (actor : Nat)
    (lease : ViewLease) (label : Nat) (call : WireCall) (history : List SourcePoint)
    (point : SourcePoint) :
    point ∈ publicCallCandidates plan actor lease label call history ↔
      ∃ event binding, (event, binding) ∈ plan.path.bindings ∧
        binding.message.label = label ∧ lease.contains plan event = true ∧ occurrenceActor? plan point = some actor ∧
        point ∉ history ∧
        match call with
        | .send schema => point = .accepted event ∧ binding.message.sender = lease.key ∧
            binding.message.payload = .wire schema
        | .receive payload => point = .committed ⟨event, .receive⟩ ∧
            binding.message.receiver = lease.key ∧ binding.message.payload = payload := by
  cases call <;>
    simp [publicCallCandidates, publicMessageContract, List.mem_filterMap, Prod.exists,
      and_assoc] <;> grind

/-- Admission within one finite selected path. The public call does not choose
an event index. A later ready occurrence takes priority over a blocked alias. -/
def publicCallVerdict (plan : SourceOccurrenceCut source) (actor : Nat)
    (lease : ViewLease) (label : Nat) (call : WireCall) (history : List SourcePoint) : PollVerdict :=
  let entries := publicCallCandidates plan actor lease label call history
  if entries.any (enabled plan history) then .ready
  else if entries.any (fun p => decide (verdict plan actor [lease] p history ≠ .unavailable)) then .pending
  else .unavailable

/-- Rejection excludes every independent continuation of every matching
occurrence in this path, including matching occurrences at different peers. -/
theorem public_call_exact (plan : SourceOccurrenceCut source) (actor : Nat)
    (lease : ViewLease) (label : Nat) (call : WireCall) (history : List SourcePoint) :
    let entries := publicCallCandidates plan actor lease label call history
    let result := publicCallVerdict plan actor lease label call history
    let ready := ∃ p ∈ entries, enabled plan history p = true
    let reachable := ∃ p ∈ entries, CanResume plan actor [lease] p history
    (result = .ready ↔ ready) ∧
    (result = .pending ↔ ¬ ready ∧ reachable) ∧
    (result = .unavailable ↔ ∀ p ∈ entries, ¬ CanResume plan actor [lease] p history) := by
  let entries := publicCallCandidates plan actor lease label call history
  let available := entries.any (enabled plan history)
  let resumable := entries.any fun p => decide (verdict plan actor [lease] p history ≠ .unavailable)
  have readyAt : available = true ↔ ∃ p ∈ entries, enabled plan history p = true :=
    List.any_eq_true
  have resumeAt : resumable = true ↔ ∃ p ∈ entries, CanResume plan actor [lease] p history := by
    change (entries.any _) = true ↔ _
    simp only [List.any_eq_true, decide_eq_true_eq]
    apply exists_congr
    intro p
    exact and_congr_right fun _ =>
      (not_congr (verdict_exact plan actor [lease] p history).2.2).trans
        ⟨Classical.not_not.mp, fun h deny => deny h⟩
  have immediate : available = true → resumable = true := by
    intro yes
    obtain ⟨p, member, h⟩ := readyAt.mp yes
    exact resumeAt.mpr ⟨p, member, .now h⟩
  have exhausted : (∀ p ∈ entries, ¬ CanResume plan actor [lease] p history) ↔ ¬ (resumable = true) := by
    rw [resumeAt]
    exact ⟨fun h ⟨p, member, path⟩ => h p member path, fun h p member path => h ⟨p, member, path⟩⟩
  let result : PollVerdict := if available then .ready else if resumable then .pending else .unavailable
  change (result = .ready ↔ _) ∧ (result = .pending ↔ _) ∧ (result = .unavailable ↔ _)
  rw [← readyAt, ← resumeAt, exhausted]
  dsimp only [result]
  cases h : available <;> cases k : resumable <;> simp_all

#print axioms public_candidates_exact
#print axioms public_call_exact
#print axioms verdict_exact

/-- A caller cannot wait for a mandatory preceding action which itself needs
a view held by that caller. This holds through every independent continuation. -/
theorem held_predecessor_blocks {plan : SourceOccurrenceCut source}
    (member : prior ∈ sourceVisitPoints plan.path.bindings)
    (edge : sourcePointDepends plan.expanded plan.path.bindings prior target = true)
    (heldByCaller : independent plan actor held prior = false)
    (missing : prior ∉ history) : ¬ CanResume plan actor held target history := by
  intro continuation
  induction continuation with
  | now ready =>
    have all := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp ready).1).2
    have seen := List.all_eq_true.mp all prior member
    simp [edge, occurrenceCut, missing] at seen
  | later _ free _ ih =>
    apply ih
    intro seen
    rcases List.mem_cons.mp seen with same | old
    · subst_vars
      simp [heldByCaller] at free
    · exact missing old

/-- The steps through which a pending call can resume are genuine source
transitions. The cut after each step contains exactly one new observation. -/
theorem enabled_step {plan : SourceOccurrenceCut source}
    (ready : enabled plan history point = true) :
    SourceOccurrenceStep (occurrenceCut plan history) point (occurrenceCut plan (point :: history)) := by
  obtain ⟨_, step⟩ := a_checked_source_occurrence_has_its_exact_transition _ _ (Bool.and_eq_true_iff.mp ready).1
  cases step with
  | advance present fresh prerequisites =>
    have obs : (fun p => (point :: history).contains p) = (fun p => if p = point then true else history.contains p) := by
      funext p
      by_cases same : p = point <;> simp [same]
    change SourceOccurrenceStep _ _ {plan with observed := fun p => (point :: history).contains p}
    rw [obs]
    exact .advance point present fresh prerequisites

/-- A wait witness extends the same source execution through the target.
Only independent steps may be inserted before its entry. -/
theorem CanResume.extends_execution {plan : SourceOccurrenceCut source}
    (resume : CanResume plan actor held target history)
    (prefixRun : SourceOccurrenceExecution first (occurrenceCut plan history) past) :
    ∃ middle final, (∀ p ∈ middle, independent plan actor held p = true) ∧
      SourceOccurrenceExecution first final (past ++ middle ++ [target]) := by
  induction resume generalizing past with
  | now ready =>
    exact ⟨[], _, by simp, by simpa using prefixRun.tail (enabled_step ready)⟩
  | @later history point ready free _ ih =>
    obtain ⟨middle, final, independentSteps, execution⟩ := ih (prefixRun.tail (enabled_step ready))
    refine ⟨point :: middle, final, ?_, ?_⟩
    · intro p member
      obtain rfl | later := List.mem_cons.mp member
      · exact free
      · exact independentSteps _ later
    · simpa [List.append_assoc] using execution

#print axioms CanResume.extends_execution
#print axioms held_predecessor_blocks
#print axioms enabled_step

/-- Replay chronological operations against each performer's projected history.
Sender publication and receiver commit also require a transport acceptance.
A resumed send must satisfy the roll guard at its own prefix. -/
def ProjectedSuffix (plan : SourceOccurrenceCut source) :
    List SourcePoint → List SourcePoint → Prop
  | _, [] => True
  | history, point :: rest =>
      (∃ actor, let localHistory := occurrenceLocalHistory plan actor history
        occurrenceLocalReady plan actor localHistory point = true ∧
        occurrenceWireReady history point = true ∧
        (occurrenceCut plan localHistory).resumptionReady point = true) ∧
      ProjectedSuffix plan (point :: history) rest

private theorem projected_cons_iff (plan : SourceOccurrenceCut source)
    (history : List SourcePoint) (point : SourcePoint) (rest : List SourcePoint) :
    ProjectedSuffix plan history (point :: rest) ↔
      enabled plan history point = true ∧ ProjectedSuffix plan (point :: history) rest := by
  rw [ProjectedSuffix]
  apply and_congr_left
  intro _
  have localGuard (actor : Nat)
      (ready : occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor history) point = true) :
      (occurrenceCut plan history).resumptionReady point =
        (occurrenceCut plan (occurrenceLocalHistory plan actor history)).resumptionReady point := by
    have member : point ∈ occurrenceLocalDomain plan actor :=
      List.contains_iff_mem.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp ready).1).1
    exact a_resumption_query_uses_only_its_actor_history plan history point
      (of_decide_eq_true (List.mem_filter.mp member).2)
  rw [enabled, Bool.and_eq_true, projected_occurrence_readiness_is_exact]
  constructor
  · rintro ⟨actor, localReady, wire, resumed⟩
    exact ⟨⟨actor, localReady, wire⟩, (localGuard actor localReady).trans resumed⟩
  · rintro ⟨⟨actor, localReady, wire⟩, resumed⟩
    exact ⟨actor, localReady, wire, (localGuard actor localReady).symm.trans resumed⟩

/-- Completeness as well as soundness: an independently executable projected
suffix cannot be lost by the wait search. Only the intervening actions have to
avoid the caller's held views; the final point is the caller's own operation. -/
theorem canResume_iff_projected_suffix {plan : SourceOccurrenceCut source} :
    CanResume plan actor held target history ↔ ∃ middle,
      (∀ p ∈ middle, independent plan actor held p = true) ∧
      ProjectedSuffix plan history (middle ++ [target]) := by
  constructor
  · intro can
    induction can with
    | now ready =>
      exact ⟨[], by simp, (projected_cons_iff ..).mpr ⟨ready, trivial⟩⟩
    | later ready free _ ih =>
      obtain ⟨middle, freeRest, suffix⟩ := ih
      exact ⟨_ :: middle, by simpa using And.intro free freeRest,
        (projected_cons_iff ..).mpr ⟨ready, suffix⟩⟩
  · rintro ⟨middle, free, suffix⟩
    induction middle generalizing history with
    | nil => exact .now ((projected_cons_iff ..).mp suffix).1
    | cons p rest ih =>
      obtain ⟨ready, tailReady⟩ := (projected_cons_iff ..).mp suffix
      exact .later ready (free p (by simp))
        (ih (fun q member => free q (List.mem_cons_of_mem _ member)) tailReady)

/-- A recoverable refusal rules out every legal projected schedule to every
matching entry. Conversely, if all such schedules are impossible, the
reference admission returns unavailable. No Rust verdict is a premise. -/
theorem public_unavailable_iff_no_projected_suffix (plan : SourceOccurrenceCut source)
    (actor : Nat) (lease : ViewLease) (label : Nat) (call : WireCall) (history : List SourcePoint) :
    publicCallVerdict plan actor lease label call history = .unavailable ↔
      ¬ ∃ point ∈ publicCallCandidates plan actor lease label call history, ∃ middle,
        (∀ p ∈ middle, independent plan actor [lease] p = true) ∧
        ProjectedSuffix plan history (middle ++ [point]) := by
  rw [(public_call_exact plan actor lease label call history).2.2]
  constructor
  · rintro all ⟨point, member, suffix⟩
    exact all point member (canResume_iff_projected_suffix.mpr suffix)
  · intro none point member resume
    exact none ⟨point, member, canResume_iff_projected_suffix.mp resume⟩

/-- A sufficient reason to refuse a contract in a particular replay. Each
alias must have a still-missing, mandatory source predecessor which cannot
run during this borrow. No public poll outcome is an input. -/
def blockedByHeldPrerequisite (plan : SourceOccurrenceCut source) (actor : Nat)
    (lease : ViewLease) (label : Nat) (call : WireCall) (history : List SourcePoint) : Bool :=
  (publicCallCandidates plan actor lease label call history).all fun target =>
    (sourceVisitPoints plan.path.bindings).any fun prior =>
      !history.contains prior && sourcePointDepends plan.expanded plan.path.bindings prior target &&
        !independent plan actor [lease] prior

theorem checked_blockers_exclude_projected_progress (plan : SourceOccurrenceCut source)
    (blocked : blockedByHeldPrerequisite plan actor lease label call history = true)
    (member : target ∈ publicCallCandidates plan actor lease label call history) :
    ¬ ∃ middle, (∀ p ∈ middle, independent plan actor [lease] p = true) ∧
      ProjectedSuffix plan history (middle ++ [target]) := by
  obtain ⟨prior, present, evidence⟩ := List.any_eq_true.mp (List.all_eq_true.mp blocked target member)
  simp only [Bool.and_eq_true, Bool.not_eq_true'] at evidence
  have missing : prior ∉ history := by simpa using evidence.1.1
  exact fun suffix => held_predecessor_blocks present evidence.1.2 evidence.2 missing
    (canResume_iff_projected_suffix.mpr suffix)

/-- Different completions can require different blockers. Only facts proved
for every replay of the same physical prefix can justify rejecting across an
open choice. Histories include physical acceptance before sender publication. -/
theorem checked_blockers_exclude_progress_on_every_replay
    {source : Source} {images : Nat → RustDescriptorImage} {choreo : Choreo}
    {word : List DistributedCallBoundary}
    (blocked : ∀ (plan : SourceOccurrenceCut source) {calls history},
      SourceOccurrenceTrace images choreo plan word calls history →
        blockedByHeldPrerequisite plan actor lease label call history = true) :
    ¬ ∃ (plan : SourceOccurrenceCut source), ∃ calls history,
      SourceOccurrenceTrace images choreo plan word calls history ∧
      ∃ target ∈ publicCallCandidates plan actor lease label call history, ∃ middle,
        (∀ p ∈ middle, independent plan actor [lease] p = true) ∧
        ProjectedSuffix plan history (middle ++ [target]) := by
  rintro ⟨plan, _, _, trace, target, member, suffix⟩
  exact checked_blockers_exclude_projected_progress plan (blocked plan trace) member suffix

#print axioms public_unavailable_iff_no_projected_suffix
#print axioms sequential_prefix_or_tail_has_no_sibling_handle
#print axioms accepted_call_preserves_fragment_membership
#print axioms checked_blockers_exclude_progress_on_every_replay

/-- One actor participates in all four sends. The global source supplies the
action IDs, paths, lanes, dependencies and the three nested roll intervals. -/
private def nestedCreditSelections? : Option (List (List Nat)) := do
  let choreo := Choreo.roll (.par (.send 0 1 0 1) (.roll (.par (.send 0 1 1 1)
    (.roll (.seq (.send 0 1 2 1) (.send 0 1 3 1))))))
  let program := canonicalProgramSource choreo
  let source ← choreo.ownershipSource?
  let normalized ← EndpointDelegation.normalize source
  let path ← normalized.paths[0]?
  let actions := source.actorCandidateActions normalized.paths 0
  let data : DecodedActorOrder := {
    source, normalized, actor := 0, markers := program.markers
    columns := {
      atomCount := program.atoms.length, routeResolverCount := 0, routeParticipantCount := 0
      scopeMarkerCount := program.markers.length
      scopeMarkerOffset := program.atoms.length * productionProgramAtomStride
      actorBindings := [], actorDecisions := [], initialActors := [] }
    rows := actions.zipIdx.map fun ((actionId, action), eventId) =>
      { eventId, actionId, view := action.endpoint.role, actorReference := action.endpoint.role }
    lanes := program.atoms.map (·.lane), dependencies := choreo.canonicalRoleDependencies 0
    eventDependencies := choreo.canonicalRoleEventDependencyRows 0 }
  let rolls ← choreo.canonicalActorRollRows? 0
  let [parent, outer, inner] := rolls | none
  let kept := fun credits entry =>
    (sourceIndependentRetainedChildren data path rolls credits parent entry).map (·.scope)
  let both := [outer.scope, inner.scope]
  return [kept both 0, kept both 1, kept [inner.scope] 1, kept both.reverse 0, kept both.reverse 1]

/-- Entry zero is beside the outer roll. Entry one is inside it and beside
the inner roll. Reversing the credit inventory gives the same results. -/
theorem nested_credit_selection_before_independence :
    nestedCreditSelections? = some [[2], [], [4], [2], []] := by decide

private theorem preserved_child_has_pre_cut_credit
    (member : child ∈ sourceIndependentRetainedChildren data path rolls credits parent entry) :
    child.scope ∈ credits := by
  unfold sourceIndependentRetainedChildren at member
  split at member
  · simp at member
  · have allowed := (List.mem_filter.mp member).2
    simp only [Bool.and_eq_true, and_assoc] at allowed
    exact List.contains_iff_mem.mp allowed.1

private theorem reset_done_at_any_row (before : ActorProgressObservation)
    (parent : DecodedRollRow) (choices : List (Nat × Option RouteArm))
    (preserved : List DecodedRollRow) (row : Nat) :
    (resetObservedActor before parent choices preserved).doneState.done row =
      if preserved.any (fun child => child.eventStart ≤ row &&
          row < child.eventStart + child.eventLength) then before.doneState.done row
      else if parent.eventStart ≤ row && row < parent.eventStart + parent.eventLength
        then false else before.doneState.done row := by
  simp only [resetObservedActor, ActorProgressObservation.doneState,
    ActorProgressObservation.progress, List.getElem?_map, List.getElem?_zipIdx, Nat.zero_add]
  cases before.steps[row]? <;> simp

/-- A sticky completion bit is created for the restarting scope. An existing
bit inside that scope survives precisely when its roll is nested in a
preserved child. An enclosing or disjoint roll keeps its previous bit. -/
theorem sticky_completion_after_source_restart (rolls : List DecodedRollRow)
    (parent : DecodedRollRow) (retained : List Nat) (preserved : List DecodedRollRow)
    (scope : Nat) :
    scope ∈ retainedAfterSourceRestart rolls parent retained preserved ↔
      ∃ roll ∈ rolls, roll.scope = scope ∧
        (roll.scope = parent.scope ∨ (roll.scope ∈ retained ∧
          (roll.nestedWithin parent = false ∨
            ∃ child ∈ preserved, roll.nestedWithin child = true))) := by
  unfold retainedAfterSourceRestart
  constructor
  · intro member
    obtain ⟨roll, present, emitted⟩ := List.mem_filterMap.mp member
    split at emitted
    next kept =>
      refine ⟨roll, present, Option.some.inj emitted, ?_⟩
      simpa only [Bool.or_eq_true, Bool.and_eq_true, beq_iff_eq,
        List.contains_iff_mem, Bool.not_eq_true', List.any_eq_true] using kept
    next => cases emitted
  · rintro ⟨roll, present, rfl, kept⟩
    refine List.mem_filterMap.mpr ⟨roll, present, ?_⟩
    have selected : (roll.scope == parent.scope || (retained.contains roll.scope &&
        (!roll.nestedWithin parent || preserved.any fun child => roll.nestedWithin child))) = true := by
      simpa only [Bool.or_eq_true, Bool.and_eq_true, beq_iff_eq,
        List.contains_iff_mem, Bool.not_eq_true', List.any_eq_true] using kept
    exact if_pos selected

/-- A successful prospective body query derives the reset state from the
pre-operation credit bits and the source path. The same preserved children
determine the done bit of every event and the sticky completion of every roll.
The query has no post-operation observation as input. -/
theorem a_queried_parent_reset_preserves_exactly_the_precredited_children
    {image : RustDescriptorImage} {choreo : Choreo} {before base : ActorProgressObservation}
    {credits : List Nat} {previousPath nextPath scope entry : Nat} {parent : DecodedRollRow}
    (compiled : observedActorBodyQueries? image choreo before credits = some query)
    (started : query previousPath nextPath scope entry = some (parent, base)) :
    ∃ data previous rolls, image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[previousPath]? = some previous ∧ image.decodeRollRows? = some rolls ∧
      parent ∈ rolls ∧ parent.scope = scope ∧
      let preserved := sourceIndependentRetainedChildren data previous rolls credits parent entry
      (∀ child ∈ preserved, child.scope ∈ credits) ∧
      (∀ row, base.doneState.done row =
        if preserved.any (fun child => child.eventStart ≤ row &&
            row < child.eventStart + child.eventLength) then before.doneState.done row
        else if parent.eventStart ≤ row && row < parent.eventStart + parent.eventLength
          then false else before.doneState.done row) ∧
      (∀ retained id, id ∈ retainedAfterSourceRestart rolls parent retained preserved ↔
        ∃ roll ∈ rolls, roll.scope = id ∧
          (roll.scope = parent.scope ∨ (roll.scope ∈ retained ∧
            (roll.nestedWithin parent = false ∨
              ∃ child ∈ preserved, roll.nestedWithin child = true)))) := by
  unfold observedActorBodyQueries? at compiled
  obtain ⟨data, dataAt, compiled⟩ := Option.bind_eq_some_iff.mp compiled
  obtain ⟨rolls, rollsAt, compiled⟩ := Option.bind_eq_some_iff.mp compiled
  dsimp only at compiled
  split at compiled
  next =>
    cases Option.some.inj compiled
    dsimp only at started
    obtain ⟨previous, previousAt, started⟩ := Option.bind_eq_some_iff.mp started
    obtain ⟨nextPathValue, _, started⟩ := Option.bind_eq_some_iff.mp started
    obtain ⟨chosen, chosenAt, started⟩ := Option.bind_eq_some_iff.mp started
    obtain ⟨choices, _, allowed⟩ := Option.bind_eq_some_iff.mp started
    split at allowed
    next =>
      cases Option.some.inj allowed
      have filtered : parent ∈ rolls.filter (fun row => row.scope == scope) := by
        rw [resolveUnique?_eq_some_iff.mp chosenAt]
        exact List.mem_singleton_self _
      have member := List.mem_filter.mp filtered
      exact ⟨data, previous, rolls, dataAt, previousAt, rollsAt, member.1,
        beq_iff_eq.mp member.2, fun _ h => preserved_child_has_pre_cut_credit h,
        reset_done_at_any_row before parent choices _,
        fun retained id => sticky_completion_after_source_restart rolls parent retained _ id⟩
    next => simp at allowed
  next => simp at compiled

#print axioms a_queried_parent_reset_preserves_exactly_the_precredited_children

end Hibana.AlternateVisitWait
