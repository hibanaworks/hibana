import Std

/-!
Narrow immutable reverse-parent lookup equivalence. No tree or reachability
hypothesis is used. `Entry.invalid` models any invariant failure while reading
one legacy fact; the scan must stop at its first match, even if a later fact is
invalid. Source slots are flattened in exactly (slot, arm 0, arm 1) order.

Source bridge obligations are documented at the end of this file. In particular,
this is not a proof of Rust decoding, unsafe pointer access, or a const checker.
-/
namespace Hibana.PassiveParentIndex

abbrev Scope := Nat

structure Key where
  parent : Scope
  arm : Nat
  deriving DecidableEq, Repr

structure Row where
  key : Key
  child : Option Scope
  deriving DecidableEq, Repr

inductive Entry where
  | invalid
  | missing
  | row (value : Row)
  deriving DecidableEq, Repr

inductive Result where
  | invalid
  | absent
  | found (key : Key)
  deriving DecidableEq, Repr

abbrev Owner := Scope → Option Key

/-- The legacy first-match scan, including both failure and early return. -/
def scan (query : Scope) : List Entry → Result
  | [] => .absent
  | .invalid :: _ => .invalid
  | .missing :: tail => scan query tail
  | .row r :: tail =>
      if r.child = some query then .found r.key else scan query tail

/-- The parent/arm lookup used only behind a successful certificate. -/
def parentRow (key : Key) : List Entry → Option Row
  | [] => none
  | .invalid :: tail | .missing :: tail => parentRow key tail
  | .row r :: tail => if r.key = key then some r else parentRow key tail

/-- Ownership alone is insufficient: the exact passive edge is checked. -/
def direct (owner : Owner) (query : Scope) (facts : List Entry) : Result :=
  match owner query with
  | none => .absent
  | some key =>
      match parentRow key facts with
      | none => .absent
      | some r => if r.child = some query then .found key else .absent

def keys : List Entry → List Key
  | [] => []
  | .invalid :: tail | .missing :: tail => keys tail
  | .row r :: tail => r.key :: keys tail

def goodEntry (owner : Owner) : Entry → Bool
  | .invalid => false
  | .missing => true
  | .row r => match r.child with
      | none => true
      | some child => owner child == some r.key

/-- Full validation of every scanned fact, unique parent/arm keys, and ownership. -/
def certificate (owner : Owner) (facts : List Entry) : Bool :=
  facts.all (goodEntry owner) && decide (keys facts |>.Nodup)

def hybrid (owner : Owner) (query : Scope) (facts : List Entry) : Result :=
  if certificate owner facts then direct owner query facts else scan query facts

 theorem parentRow_mem {facts : List Entry} {key : Key} {r : Row}
    (h : parentRow key facts = some r) : .row r ∈ facts ∧ r.key = key := by
  induction facts with
  | nil => simp [parentRow] at h
  | cons head tail ih =>
      cases head with
      | invalid =>
          obtain ⟨hm, hk⟩ := ih h
          exact ⟨List.mem_cons_of_mem _ hm, hk⟩
      | missing =>
          obtain ⟨hm, hk⟩ := ih h
          exact ⟨List.mem_cons_of_mem _ hm, hk⟩
      | row current =>
          by_cases hk : current.key = key
          · simp [parentRow, hk] at h
            subst r
            exact ⟨by simp, hk⟩
          · have ht : parentRow key tail = some r := by simpa [parentRow, hk] using h
            obtain ⟨hm, hr⟩ := ih ht
            exact ⟨List.mem_cons_of_mem _ hm, hr⟩

 theorem key_mem {facts : List Entry} {r : Row}
    (h : .row r ∈ facts) : r.key ∈ keys facts := by
  induction facts with
  | nil => simp at h
  | cons head tail ih =>
      cases head with
      | invalid => exact ih (by simpa using h)
      | missing => exact ih (by simpa using h)
      | row current =>
          simp only [List.mem_cons, Entry.row.injEq] at h
          rcases h with he | hm
          · subst current
            simp [keys]
          · exact List.mem_cons_of_mem _ (ih hm)

 theorem parentRow_of_mem {facts : List Entry} {r : Row}
    (unique : (keys facts).Nodup) (h : .row r ∈ facts) :
    parentRow r.key facts = some r := by
  induction facts with
  | nil => simp at h
  | cons head tail ih =>
      cases head with
      | invalid =>
          exact ih unique (by simpa using h)
      | missing =>
          exact ih unique (by simpa using h)
      | row current =>
          have hu : current.key ∉ keys tail ∧ (keys tail).Nodup := by
            simpa [keys] using unique
          simp only [List.mem_cons, Entry.row.injEq] at h
          rcases h with eq | hm
          · subst current
            simp [parentRow]
          · have ne : current.key ≠ r.key := by
              intro eq
              apply hu.1
              rw [eq]
              exact key_mem hm
            simp [parentRow, ne, ih hu.2 hm]

 theorem good_owner {owner : Owner} {facts : List Entry} {r : Row} {q : Scope}
    (good : facts.all (goodEntry owner) = true)
    (mem : .row r ∈ facts) (child : r.child = some q) : owner q = some r.key := by
  have h := List.all_eq_true.mp good (.row r) mem
  simpa [goodEntry, child] using h

 theorem scan_of_no_child {owner : Owner} {facts : List Entry} {q : Scope}
    (good : facts.all (goodEntry owner) = true)
    (noChild : ∀ r, .row r ∈ facts → r.child ≠ some q) :
    scan q facts = .absent := by
  induction facts with
  | nil => rfl
  | cons head tail ih =>
      have hg : goodEntry owner head = true ∧ tail.all (goodEntry owner) = true := by
        simpa using good
      cases head with
      | invalid => simp [goodEntry] at hg
      | missing =>
          exact ih hg.2 (fun r hr => noChild r (List.mem_cons_of_mem _ hr))
      | row r =>
          have hn := noChild r (by simp)
          simp only [scan, hn, ↓reduceIte]
          exact ih hg.2 (fun r hr => noChild r (List.mem_cons_of_mem _ hr))

 theorem scan_of_child {owner : Owner} {facts : List Entry} {q : Scope} {r : Row}
    (good : facts.all (goodEntry owner) = true)
    (mem : .row r ∈ facts) (child : r.child = some q) :
    scan q facts = .found r.key := by
  induction facts with
  | nil => simp at mem
  | cons head tail ih =>
      have hg : goodEntry owner head = true ∧ tail.all (goodEntry owner) = true := by
        simpa using good
      cases head with
      | invalid => simp [goodEntry] at hg
      | missing => exact ih hg.2 (by simpa using mem)
      | row current =>
          by_cases hc : current.child = some q
          · have ho1 := good_owner good (by simp : .row current ∈ .row current :: tail) hc
            have ho2 := good_owner good mem child
            have hk : current.key = r.key := Option.some.inj (ho1.symm.trans ho2)
            simp [scan, hc, hk]
          · simp only [scan, hc, ↓reduceIte]
            apply ih hg.2
            simp only [List.mem_cons, Entry.row.injEq] at mem
            rcases mem with he | hm
            · subst current
              exact False.elim (hc child)
            · exact hm

/-- Arbitrary finite relation, arbitrary query; no tree assumption. -/
 theorem certified_direct_equals_scan (owner : Owner) (facts : List Entry) (q : Scope)
    (cert : certificate owner facts = true) : direct owner q facts = scan q facts := by
  have hc : facts.all (goodEntry owner) = true ∧ (keys facts).Nodup := by
    simpa [certificate] using cert
  by_cases existsChild : ∃ r, .row r ∈ facts ∧ r.child = some q
  · obtain ⟨r, hm, he⟩ := existsChild
    have ho := good_owner hc.1 hm he
    have hp := parentRow_of_mem hc.2 hm
    have hs := scan_of_child hc.1 hm he
    simp [direct, ho, hp, he, hs]
  · have hn : ∀ r, .row r ∈ facts → r.child ≠ some q := by
      intro r hr he
      exact existsChild ⟨r, hr, he⟩
    rw [scan_of_no_child hc.1 hn]
    unfold direct
    cases ho : owner q with
    | none => rfl
    | some key =>
        cases hp : parentRow key facts with
        | none => simp [hp]
        | some r =>
            have hm := (parentRow_mem hp).1
            simp [hp, hn r hm]

/-- Total equivalence, including malformed and uncertified inputs. -/
 theorem hybrid_equals_scan (owner : Owner) (facts : List Entry) (q : Scope) :
    hybrid owner q facts = scan q facts := by
  unfold hybrid
  split
  · exact certified_direct_equals_scan owner facts q ‹_›
  · rfl

/-- The Rust checker additionally validates every owner/conflict row and scope ID.
These stronger source checks are allowed to reject extra descriptors, but they
must never force eager legacy errors while deciding eligibility. -/
def sourceCertificate (allOwnerReadsValid : Bool) (scopeIds : List Scope)
    (owner : Owner) (facts : List Entry) : Bool :=
  allOwnerReadsValid && decide scopeIds.Nodup && certificate owner facts

def sourceHybrid (allOwnerReadsValid : Bool) (scopeIds : List Scope)
    (owner : Owner) (query : Scope) (facts : List Entry) : Result :=
  if sourceCertificate allOwnerReadsValid scopeIds owner facts then
    direct owner query facts
  else scan query facts

/-- Full guarded hybrid theorem; rejecting an invalid unused owner also falls back. -/
 theorem source_hybrid_equals_scan (allOwnerReadsValid : Bool) (scopeIds : List Scope)
    (owner : Owner) (facts : List Entry) (q : Scope) :
    sourceHybrid allOwnerReadsValid scopeIds owner q facts = scan q facts := by
  unfold sourceHybrid
  split
  · have hc : allOwnerReadsValid = true ∧ scopeIds.Nodup ∧
          certificate owner facts = true := by
        simpa [sourceCertificate, and_assoc] using ‹sourceCertificate _ _ _ _ = true›
    exact certified_direct_equals_scan owner facts q hc.2.2
  · rfl

 theorem invalid_owner_forces_fallback (scopeIds : List Scope)
    (owner : Owner) (facts : List Entry) (q : Scope) :
    sourceHybrid false scopeIds owner q facts = scan q facts := by
  simp [sourceHybrid, sourceCertificate]

/-- Actual owner decoding can fail even when its row has no passive edges. -/
inductive OwnerRead where
  | invalid
  | value (owner : Option Key)
  deriving DecidableEq, Repr

abbrev OwnerReads := Scope → OwnerRead

def ownerValue (reads : OwnerReads) : Owner := fun q =>
  match reads q with
  | .invalid => none
  | .value value => value

def readDirect (reads : OwnerReads) (query : Scope) (facts : List Entry) : Result :=
  match reads query with
  | .invalid => .invalid
  | .value value => direct (fun _ => value) query facts

def checkedHybrid (allOwnerReadsValid : Bool) (scopeIds : List Scope)
    (reads : OwnerReads) (query : Scope) (facts : List Entry) : Result :=
  if sourceCertificate allOwnerReadsValid scopeIds (ownerValue reads) facts then
    readDirect reads query facts
  else scan query facts

 theorem readDirect_safe (reads : OwnerReads) (facts : List Entry) (q : Scope)
    (safe : reads q ≠ .invalid) :
    readDirect reads q facts = direct (ownerValue reads) q facts := by
  cases h : reads q with
  | invalid => exact False.elim (safe h)
  | value v => simp [readDirect, ownerValue, direct, h]

/-- Explicit owner-read failure model. `sound` is precisely the source bridge
obligation for the full conflict-table validator, including unused conflicts.
No assumption on failed certificates or invalid reads in fallback is needed. -/
 theorem checked_hybrid_equals_scan (allOwnerReadsValid : Bool) (scopeIds : List Scope)
    (reads : OwnerReads) (facts : List Entry) (q : Scope)
    (sound : allOwnerReadsValid = true → ∀ query, reads query ≠ .invalid) :
    checkedHybrid allOwnerReadsValid scopeIds reads q facts = scan q facts := by
  unfold checkedHybrid
  split
  · have hc : allOwnerReadsValid = true ∧ scopeIds.Nodup ∧
          certificate (ownerValue reads) facts = true := by
        simpa [sourceCertificate, and_assoc] using ‹sourceCertificate _ _ _ _ = true›
    rw [readDirect_safe reads facts q (sound hc.1 q)]
    exact certified_direct_equals_scan (ownerValue reads) facts q hc.2.2
  · rfl

/-- Even one malformed fact anywhere disables the fast path. -/
 theorem invalid_forces_fallback (owner : Owner) (facts : List Entry)
    (bad : .invalid ∈ facts) : certificate owner facts = false := by
  cases h : certificate owner facts with
  | false => rfl
  | true =>
      have hc : facts.all (goodEntry owner) = true ∧ (keys facts).Nodup := by
        simpa [certificate] using h
      have allGood := hc.1
      have hg := List.all_eq_true.mp allGood .invalid bad
      simp [goodEntry] at hg

/-- The certificate guarantees at most one parent/arm for any present child. -/
 theorem same_child_same_owner {owner : Owner} {facts : List Entry} {a b : Row} {q : Scope}
    (cert : certificate owner facts = true)
    (ha : .row a ∈ facts) (hb : .row b ∈ facts)
    (ca : a.child = some q) (cb : b.child = some q) : a.key = b.key := by
  have hc : facts.all (goodEntry owner) = true ∧ (keys facts).Nodup := by
    simpa [certificate] using cert
  have hg := hc.1
  exact Option.some.inj ((good_owner hg ha ca).symm.trans (good_owner hg hb cb))

/-!
A small executable source-validation layer makes the forward-slot and
malformed cases non-vacuous. `otherChecks` abstracts the additional byte bounds,
route-scope decoding, row presence, event ranges and accumulated lane-step checks.
It MUST reflect every validation performed by the legacy getter, not just checks
on rows encountered by a particular query. The real source getter checks owner
agreement and forward slot order before returning a present child.
-/
structure RawRow where
  parentSlot : Nat
  key : Key
  child : Option (Scope × Nat)
  otherChecks : Bool := true
  deriving DecidableEq, Repr

def validate (owner : Owner) (r : RawRow) : Entry :=
  if r.otherChecks && decide (r.key.arm < 2) then
    match r.child with
    | none => .row ⟨r.key, none⟩
    | some (child, childSlot) =>
        if decide (r.parentSlot < childSlot) && decide (child ≠ r.key.parent) &&
            (owner child == some r.key) then
          .row ⟨r.key, some child⟩
        else .invalid
  else .invalid

/-- Back edges (including self edges) cannot be certified as source facts. -/
 theorem backward_invalid (owner : Owner) (r : RawRow) (child childSlot : Nat)
    (he : r.child = some (child, childSlot)) (backward : childSlot ≤ r.parentSlot) :
    validate owner r = .invalid := by
  unfold validate
  split
  · simp [he, show ¬ r.parentSlot < childSlot by omega]
  · rfl

/-- Slot order strictly advances along every source-valid passive edge. -/
 theorem forward_path_progress (slots : Nat → Nat) (length : Nat)
    (forward : ∀ i, i < length → slots i < slots (i + 1)) :
    slots 0 + length ≤ slots length := by
  induction length with
  | zero => omega
  | succ n ih =>
      have hp := ih (fun i hi => forward i (by omega))
      have hn := forward n (by omega)
      omega

/-- Arbitrary finite cycles cannot consist entirely of forward source edges.
Combined with backward_invalid, a consistently slotted cycle contains a fact
that disables indexing. This does not assume a tree or even connectedness. -/
 theorem cycle_not_all_forward (slots : Nat → Nat) (length : Nat)
    (nonempty : 0 < length) (closed : slots length = slots 0) :
    ¬ (∀ i, i < length → slots i < slots (i + 1)) := by
  intro forward
  have hp := forward_path_progress slots length forward
  omega

/-- Unique scope IDs imply unique flattened (scope, 0)/(scope, 1) row keys. -/
def scopeKeys (scopes : List Scope) : List Key :=
  scopes.flatMap (fun scope => [⟨scope, 0⟩, ⟨scope, 1⟩])

 theorem scopeKeys_nodup (scopes : List Scope) (h : scopes.Nodup) :
    (scopeKeys scopes).Nodup := by
  induction scopes with
  | nil => simp [scopeKeys]
  | cons scope tail ih =>
      have ht : scope ∉ tail ∧ tail.Nodup := by simpa using h
      have hzero : (Key.mk scope 0) ∉ scopeKeys tail := by
        simp only [scopeKeys, List.mem_flatMap]
        rintro ⟨s, hs, hk⟩
        have he : scope = s := by simpa using hk
        exact ht.1 (he ▸ hs)
      have hone : (Key.mk scope 1) ∉ scopeKeys tail := by
        simp only [scopeKeys, List.mem_flatMap]
        rintro ⟨s, hs, hk⟩
        have he : scope = s := by simpa using hk
        exact ht.1 (he ▸ hs)
      change (Key.mk scope 0 :: Key.mk scope 1 :: scopeKeys tail).Nodup
      exact List.nodup_cons.mpr ⟨by simpa using hzero,
        List.nodup_cons.mpr ⟨hone, ih ht.2⟩⟩

/- Non-vacuous kernel-evaluated witnesses. -/
def k01 : Key := ⟨10, 1⟩
def k10 : Key := ⟨20, 0⟩
def owners : Owner := fun q => if q = 20 then some k01 else none

def present : List Entry := [.row ⟨⟨10, 0⟩, none⟩, .row ⟨k01, some 20⟩]
example : certificate owners present = true := by decide
example : hybrid owners 20 present = .found k01 := by decide
example : hybrid owners 99 present = .absent := by decide
example : hybrid owners 20 [] = .absent := by decide

-- A child may have an owner conflict yet no passive edge: exact-edge check matters.
def noEdge : List Entry := [.row ⟨k01, none⟩]
example : certificate owners noEdge = true := by decide
example : owners 20 = some k01 := by decide
example : hybrid owners 20 noEdge = .absent := by decide

-- Duplicate parent/arm keys disable indexing, preserving legacy first-match.
def duplicate : List Entry := [.row ⟨k01, none⟩, .row ⟨k01, some 20⟩]
example : certificate owners duplicate = false := by decide
example : direct owners 20 duplicate = .absent := by decide
example : hybrid owners 20 duplicate = .found k01 := by decide

-- A present edge with the wrong owner fails source validation and falls back.
def wrongOwner : List Entry := [validate (fun _ => none) ⟨0, k01, some (20, 1), true⟩]
example : certificate (fun _ => none) wrongOwner = false := by decide
example : hybrid (fun _ => none) 20 wrongOwner = .invalid := by decide

-- A 10 -> 20 -> 10 cycle has one backwards edge. Owner consistency alone
-- would not reject it; the actual forward-slot validation does.
def cyclicOwners : Owner := fun q =>
  if q = 20 then some k01 else if q = 10 then some k10 else none
def cyclic : List Entry :=
  [validate cyclicOwners ⟨0, k01, some (20, 1), true⟩,
   validate cyclicOwners ⟨1, k10, some (10, 0), true⟩]
example : certificate cyclicOwners cyclic = false := by decide
example : hybrid cyclicOwners 20 cyclic = .found k01 := by decide
example : hybrid cyclicOwners 10 cyclic = .invalid := by decide
example : hybrid cyclicOwners 99 cyclic = .invalid := by decide

-- Owner metadata can be cyclic without any passive child edges. Such unused
-- ownership is harmless: the exact-edge checks return None, without a walk.
def unusedOwnerCycle : List Entry := [.row ⟨k01, none⟩, .row ⟨k10, none⟩]
example : sourceCertificate true [10, 20] cyclicOwners unusedOwnerCycle = true := by decide
example : sourceHybrid true [10, 20] cyclicOwners 10 unusedOwnerCycle = .absent := by decide
example : sourceHybrid true [10, 20] cyclicOwners 20 unusedOwnerCycle = .absent := by decide

-- An invalid unused owner must reject source eligibility without changing an
-- otherwise successful/absent legacy scan. It must not eagerly produce invalid.
example : sourceCertificate false [10, 20] owners noEdge = false := by decide
example : sourceHybrid false [10, 20] owners 20 noEdge = .absent := by decide
example : sourceHybrid false [10, 20] owners 20 present = .found k01 := by decide
example : sourceCertificate true [10, 10] owners present = false := by decide
example : sourceHybrid true [10, 10] owners 20 present = .found k01 := by decide

-- Explicit regression: dropping only the full-owner-read guard adds an error
-- on a harmless miss. The invalid conflict is not referenced by any child edge.
def unusedInvalidRead : OwnerReads := fun q =>
  if q = 20 then .invalid else .value none
example : readDirect unusedInvalidRead 20 noEdge = .invalid := by decide
example : scan 20 noEdge = .absent := by decide
example : certificate (ownerValue unusedInvalidRead) noEdge = true := by decide
example : checkedHybrid false [10, 20] unusedInvalidRead 20 noEdge = .absent := by decide
example : checkedHybrid true [10, 20] unusedInvalidRead 20 noEdge = .invalid := by decide
theorem dropping_owner_guard_changes_harmless_miss :
    checkedHybrid true [10, 20] unusedInvalidRead 20 noEdge ≠ scan 20 noEdge := by decide

-- Duplicate child edges with conflicting parents disable the core certificate.
-- In arbitrary decoded relations fallback still retains the original first row.
def duplicateChild : List Entry := [.row ⟨k01, some 20⟩, .row ⟨⟨30, 0⟩, some 20⟩]
example : certificate owners duplicateChild = false := by decide
example : hybrid owners 20 duplicateChild = .found k01 := by decide

-- All rows are checked for eligibility, but a failed check must not eagerly
-- execute the legacy getter or replace a successful earlier first match.
def lateMalformed : List Entry := present ++ [.invalid]
example : certificate owners lateMalformed = false := by decide
example : hybrid owners 20 lateMalformed = .found k01 := by decide
example : hybrid owners 99 lateMalformed = .invalid := by decide
example : hybrid owners 20 [.invalid, .row ⟨k01, some 20⟩] = .invalid := by decide
example : hybrid owners 99 [.missing] = .absent := by decide
example : scopeKeys [10, 20] |>.Nodup := by decide
example : ¬ (scopeKeys [10, 10]).Nodup := by decide

/-!
SOURCE BRIDGE (assumptions to discharge by Rust review/tests, not Lean axioms):
1. The facts list is exactly passive_arm_child_fact_by_slot(slot, arm) for
   slot = 0..route_count-1 and arm = 0,1 in that order. A getter None is missing;
   any invariant failure is invalid; a returned row preserves both key and child.
2. The precomputed const eligibility check is non-panicking on invalid bytes and
   establishes every legacy fact can be read, every scope ID is unique, and every
   present passive child agrees with its immutable route-arm owner. It also checks
   ALL conflict rows, even unused ones (the fast path newly reads the queried
   child owner), not only conflicts reached by passive edges. It also checks
   every source validation (including childSlot > parentSlot, scope kinds, column
   bounds, nonempty packed rows, event ranges and lane-step prefix/range checks).
   Unique scopes plus exactly one row per arm imply unique keys, as proved above.
3. `owner` is a safe optional lookup on the same immutable descriptor; absent,
   wrong-kind queries, non-route-arm conflicts, and missing parents produce None.
   Malformed stored conflicts must instead make allOwnerReadsValid false so their
   new owner read cannot add an error absent in the legacy scan. The fast
   parent/arm row lookup agrees with parentRow under certified uniqueness. It may
   not treat a conflict alone as a passive edge: it must test the exact child.
4. Const-check failure stores false without raising an eager new invariant error;
   hybrid false executes the identical legacy scan, with identical order and
   early return. No malformed edge is silently skipped by this fallback.
5. Certificate and lookups refer to the same immutable bytes for their lifetime.
   The bounded ancestor walk, hop bound, and invariant failure after exhaustion
   remain unchanged. This theorem makes no whole-cursor, liveness, or performance
   claim and assumes no tree topology, connectedness, or dense scope numbering.
-/
#print axioms certified_direct_equals_scan
#print axioms hybrid_equals_scan
#print axioms source_hybrid_equals_scan
#print axioms readDirect_safe
#print axioms checked_hybrid_equals_scan
#print axioms invalid_owner_forces_fallback
#print axioms invalid_forces_fallback
#print axioms same_child_same_owner
#print axioms backward_invalid
#print axioms forward_path_progress
#print axioms cycle_not_all_forward
#print axioms scopeKeys_nodup
#print axioms dropping_owner_guard_changes_harmless_miss
end Hibana.PassiveParentIndex
