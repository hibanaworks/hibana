import Std

/-!
Immutable packed route-arm prefix elision. The model splits the eight raw bytes
into the same little-endian 16/16/16/8/8-bit fields as the Rust readers. A missing
row and a malformed/unreadable row are explicit. Only the metadata prefix and
final immutable bounds checks are elided; dynamic event decoding is out of scope.
-/
namespace Hibana.ArmRowIndex

/-- Exactly the field domains of the two packed u32 words. -/
structure RawRow where
  eventStart : Fin 65536
  eventLen : Fin 65536
  childSlot : Fin 65536
  encodedStepLen : Fin 256
  reserved : Fin 256
  deriving DecidableEq, Repr

inductive Entry where
  | missing
  | invalid
  | raw (row : RawRow)
  deriving DecidableEq, Repr

inductive Result where
  | invalid
  | found (row : RawRow)
  deriving DecidableEq, Repr

/-- PackedRouteArmRow::from_packed_parts plus packed_route_arm_row's empty check. -/
def decodePacked : Entry → Option RawRow
  | .missing | .invalid => none
  | .raw r =>
      if r.reserved.val ≠ 0 ∨
          (r.eventStart.val = 65535 ∧ r.eventLen.val = 65535) ∨
          (r.eventLen.val = 0 ∧ r.eventStart.val ≠ 0)
      then none else some r

/-- Kept separate: from_packed_parts does not check the zero-event encoded byte. -/
def laneStepLen (r : RawRow) : Option Nat :=
  if r.eventLen.val = 0 then
    if r.encodedStepLen.val = 0 then some 0 else none
  else some (r.encodedStepLen.val + 1)

def eventEnd (r : RawRow) : Nat := r.eventStart.val + r.eventLen.val

def coherent (r : RawRow) (len : Nat) : Prop := (r.eventLen.val = 0 ↔ len = 0)
instance (r : RawRow) (len : Nat) : Decidable (coherent r len) :=
  inferInstanceAs (Decidable (r.eventLen.val = 0 ↔ len = 0))

def packedAt : List Entry → Nat → Option RawRow
  | [], _ => none
  | head :: _, 0 => decodePacked head
  | _ :: tail, q + 1 => packedAt tail q

/-- Ascending predecessor reads; selected-row decoding is done separately first. -/
def prefixLen : List Entry → Nat → Option Nat
  | _, 0 => some 0
  | [], _ + 1 => none
  | head :: tail, q + 1 =>
      match decodePacked head with
      | none => none
      | some r =>
          match laneStepLen r with
          | none => none
          | some len => (prefixLen tail q).map (fun rest => len + rest)

/-- Every row and every cumulative prefix is checked, as in the const certificate. -/
def checkFrom (events steps acc : Nat) : List Entry → Bool
  | [] => true
  | head :: tail =>
      match decodePacked head with
      | none => false
      | some r =>
          match laneStepLen r with
          | none => false
          | some len =>
              decide (eventEnd r ≤ events ∧ coherent r len ∧ acc + len ≤ steps) &&
                checkFrom events steps (acc + len) tail

def certificate (events steps : Nat) (rows : List Entry) : Bool :=
  checkFrom events steps 0 rows

/-- Original order: selected packed row, all predecessors' lengths, selected
length, then final event/coherence/prefix bounds. All source failures are the
same externally visible invariant failure. -/
def original (events steps : Nat) (rows : List Entry) (q : Nat) : Result :=
  match packedAt rows q with
  | none => .invalid
  | some r =>
      match prefixLen rows q with
      | none => .invalid
      | some pre =>
          match laneStepLen r with
          | none => .invalid
          | some len =>
              if eventEnd r ≤ events ∧ coherent r len ∧ pre + len ≤ steps
              then .found r else .invalid

def direct (rows : List Entry) (q : Nat) : Result :=
  match packedAt rows q with
  | none => .invalid
  | some r => .found r

def hybrid (events steps : Nat) (rows : List Entry) (q : Nat) : Result :=
  if certificate events steps rows then direct rows q else original events steps rows q

 theorem lane_length_bound (r : RawRow) (len : Nat) (h : laneStepLen r = some len) :
    len ≤ 256 := by
  have hb := r.encodedStepLen.isLt
  unfold laneStepLen at h
  split at h
  · split at h <;> simp_all <;> omega
  · simp_all
    omega

 theorem lane_length_coherent (r : RawRow) (len : Nat) (h : laneStepLen r = some len) :
    coherent r len := by
  unfold laneStepLen at h
  unfold coherent
  split at h
  · split at h <;> simp_all <;> omega
  · simp_all
    omega

 theorem packed_missing_out_of_range (rows : List Entry) (q : Nat)
    (hq : rows.length ≤ q) : packedAt rows q = none := by
  induction rows generalizing q with
  | nil => simp [packedAt]
  | cons head tail ih =>
      cases q with
      | zero => simp at hq
      | succ q => exact ih q (by simpa using hq)

/-- A successful cumulative checker supplies totality of every selected row and
all its preceding reads, together with the exact final bound the old accessor tests. -/
 theorem checked_query (events steps acc : Nat) (rows : List Entry) (q : Nat)
    (hc : checkFrom events steps acc rows = true) (hq : q < rows.length) :
    ∃ r len pre,
      packedAt rows q = some r ∧ laneStepLen r = some len ∧
      prefixLen rows q = some pre ∧ eventEnd r ≤ events ∧ coherent r len ∧
      acc + pre + len ≤ steps := by
  induction rows generalizing acc q with
  | nil => simp at hq
  | cons head tail ih =>
      cases hd : decodePacked head with
      | none => simp [checkFrom, hd] at hc
      | some r =>
          cases hl : laneStepLen r with
          | none => simp [checkFrom, hd, hl] at hc
          | some len =>
              have both : (eventEnd r ≤ events ∧ coherent r len ∧ acc + len ≤ steps) ∧
                  checkFrom events steps (acc + len) tail = true := by
                simpa [checkFrom, hd, hl] using hc
              rcases both with ⟨⟨he, hz, hb⟩, ht⟩
              cases q with
              | zero =>
                  exact ⟨r, len, 0, by simp [packedAt, hd], hl,
                    by simp [prefixLen], he, hz, by simpa using hb⟩
              | succ q =>
                  obtain ⟨rr, ll, pp, hp, hll, hpre, hee, hzz, hbb⟩ :=
                    ih (acc + len) q ht (by simpa using hq)
                  refine ⟨rr, ll, len + pp, hp, hll, ?_, hee, hzz, ?_⟩
                  · simp [prefixLen, hd, hl, hpre]
                  · omega

 theorem certified_direct_equals_original (events steps : Nat) (rows : List Entry)
    (q : Nat) (hc : certificate events steps rows = true) :
    direct rows q = original events steps rows q := by
  by_cases hq : q < rows.length
  · obtain ⟨r, len, pre, hr, hl, hp, he, hz, hb⟩ :=
      checked_query events steps 0 rows q hc hq
    have final : eventEnd r ≤ events ∧ coherent r len ∧ pre + len ≤ steps :=
      ⟨he, hz, by simpa using hb⟩
    simp [direct, original, hr, hp, hl, final]
  · have hm := packed_missing_out_of_range rows q (by omega)
    simp [direct, original, hm]

 theorem guarded_hybrid_equals_original (events steps : Nat) (rows : List Entry)
    (q : Nat) : hybrid events steps rows q = original events steps rows q := by
  unfold hybrid
  split
  · exact certified_direct_equals_original events steps rows q (by assumption)
  · rfl

 theorem rejected_certificate_keeps_original (events steps : Nat) (rows : List Entry)
    (q : Nat) (hc : certificate events steps rows = false) :
    hybrid events steps rows q = original events steps rows q := by
  simp [hybrid, hc]

 theorem out_of_range_preserves_error (events steps : Nat) (rows : List Entry)
    (q : Nat) (hq : rows.length ≤ q) :
    direct rows q = .invalid ∧ original events steps rows q = .invalid ∧
      hybrid events steps rows q = .invalid := by
  have hm := packed_missing_out_of_range rows q hq
  simp [direct, original, hm, guarded_hybrid_equals_original]

/-- Raw length encoding alone bounds each successfully read prefix, independent
of certificate success or event range checks. -/
 theorem prefix_machine_bound (rows : List Entry) (q p : Nat)
    (hp : prefixLen rows q = some p) : p ≤ q * 256 := by
  induction rows generalizing q p with
  | nil =>
      cases q <;> simp [prefixLen] at hp
      · omega
  | cons head tail ih =>
      cases q with
      | zero => simp [prefixLen] at hp; omega
      | succ q =>
          cases hd : decodePacked head with
          | none => simp [prefixLen, hd] at hp
          | some r =>
              cases hl : laneStepLen r with
              | none => simp [prefixLen, hd, hl] at hp
              | some len =>
                  cases ht : prefixLen tail q with
                  | none => simp [prefixLen, hd, hl, ht] at hp
                  | some rest =>
                      have hr := ih q rest ht
                      have hb := lane_length_bound r len hl
                      simp [prefixLen, hd, hl, ht] at hp
                      omega

 theorem u16_rows_u8_lengths_no_u32_overflow (q pre len : Nat)
    (hq : q + 1 ≤ 65535) (hp : pre ≤ q * 256) (hl : len ≤ 256) :
    pre + len ≤ 16776960 ∧ pre + len < 4294967296 := by
  omega

 theorem source_prefix_no_u32_overflow (rows : List Entry) (q pre : Nat) (r : RawRow)
    (len : Nat) (hcount : rows.length ≤ 65535) (hq : q < rows.length)
    (hp : prefixLen rows q = some pre) (hl : laneStepLen r = some len) :
    pre + len < 4294967296 := by
  exact (u16_rows_u8_lengths_no_u32_overflow q pre len (by omega)
    (prefix_machine_bound rows q pre hp) (lane_length_bound r len hl)).2

/-- Constructor count and binary-arm checks are retained. This theorem extends
the row-index result to arbitrary query arms and fixed-width index arithmetic. -/
def queryRow (wordLimit slot arm : Nat) : Option Nat :=
  if arm < 2 ∧ slot * 2 < wordLimit ∧ slot * 2 + arm < wordLimit
  then some (slot * 2 + arm) else none

def originalQuery (wordLimit events steps : Nat) (rows : List Entry) (slot arm : Nat) : Result :=
  match queryRow wordLimit slot arm with
  | none => .invalid
  | some q => original events steps rows q

def hybridQuery (wordLimit events steps : Nat) (rows : List Entry) (slot arm : Nat) : Result :=
  match queryRow wordLimit slot arm with
  | none => .invalid
  | some q => hybrid events steps rows q

 theorem query_guards_preserve_every_outcome (wordLimit events steps : Nat)
    (rows : List Entry) (slot arm : Nat) :
    hybridQuery wordLimit events steps rows slot arm =
      originalQuery wordLimit events steps rows slot arm := by
  unfold hybridQuery originalQuery
  cases queryRow wordLimit slot arm with
  | none => rfl
  | some q => exact guarded_hybrid_equals_original events steps rows q

/-- Concrete finite rows: event ranges can have gaps and need not be contiguous. -/
def row (start len child : Nat) (enc reserved : Nat := 0) : RawRow where
  eventStart := ⟨start % 65536, Nat.mod_lt _ (by decide)⟩
  eventLen := ⟨len % 65536, Nat.mod_lt _ (by decide)⟩
  childSlot := ⟨child % 65536, Nat.mod_lt _ (by decide)⟩
  encodedStepLen := ⟨enc % 256, Nat.mod_lt _ (by decide)⟩
  reserved := ⟨reserved % 256, Nat.mod_lt _ (by decide)⟩

def zeroRow := row 0 0 65535
def leftRow := row 2 3 65535 1
def rightRow := row 10 2 65535 0
def goodRows : List Entry := [.raw zeroRow, .raw leftRow, .raw rightRow]

example : certificate 12 3 goodRows = true := by decide
example : original 12 3 goodRows 0 = .found zeroRow := by decide
example : original 12 3 goodRows 2 = .found rightRow := by decide
example : hybrid 12 3 goodRows 3 = .invalid := by decide
example : certificate 0 0 [] = true := by decide
example : original 0 0 [] 0 = .invalid := by decide

/-- A missing or malformed predecessor must not disappear behind an unguarded fast path. -/
def badReserved := row 0 1 65535 0 1
def badZeroCode := row 0 0 65535 1
def badPredecessor : List Entry := [.raw badReserved, .raw rightRow]

example : certificate 12 3 badPredecessor = false := by decide
example : original 12 3 badPredecessor 1 = .invalid := by decide
example : direct badPredecessor 1 = .found rightRow := by decide
example : original 12 3 [.missing, .raw rightRow] 1 = .invalid := by decide
example : original 12 3 [.invalid, .raw rightRow] 1 = .invalid := by decide
example : decodePacked (.raw badZeroCode) = some badZeroCode := by decide
example : laneStepLen badZeroCode = none := by decide
example : original 12 3 [.raw badZeroCode, .raw rightRow] 1 = .invalid := by decide
example : original 12 3 [.raw badZeroCode] 0 = .invalid := by decide
example : direct [.raw badZeroCode] 0 = .found badZeroCode := by decide

/-- An invalid later row does not invalidate the original earlier-row success. -/
def badLater : List Entry := [.raw rightRow, .raw badReserved]
example : certificate 12 1 badLater = false := by decide
example : original 12 1 badLater 0 = .found rightRow := by decide
example : hybrid 12 1 badLater 0 = .found rightRow := by decide

/-- Each row individually fits, but the cumulative prefix can overflow the declared rows. -/
def prefixOverflow : List Entry := [.raw leftRow, .raw rightRow]
example : certificate 12 2 prefixOverflow = false := by decide
example : original 12 2 prefixOverflow 0 = .found leftRow := by decide
example : original 12 2 prefixOverflow 1 = .invalid := by decide
example : direct prefixOverflow 1 = .found rightRow := by decide
example : hybrid 12 2 prefixOverflow 1 = .invalid := by decide

example : certificate 11 3 goodRows = false := by decide
example : original 11 3 goodRows 2 = .invalid := by decide
example : originalQuery 4294967296 12 3 goodRows 0 2 = .invalid := by decide
example : hybridQuery 4294967296 12 3 goodRows 2147483648 0 = .invalid := by decide

/-!
Reviewed source bridge (not extraction):
1. Fin field domains model little-endian packed reader splits exactly. Rust raw
   pointer/column-bound safety comes from constructor validation of the same
   immutable bytes; it is not inferred from this list model.
2. decodePacked models reserved bits, the all-ones event sentinel, canonical
   zero range, and packed-row decoding. laneStepLen models its distinct check.
3. checkFrom models passive_parent_rows_are_coherent's complete cumulative
   arm loop. Its additional scope/owner/edge predicates only strengthen the gate.
4. On the fast branch only the selected packed decoder remains. The old accessor
   remains unchanged for a false certificate and query/arm guards remain shared.
5. The bit is bound to RoleImageRef's immutable bytes and columns. Arbitrary
   RoleLaneImage construction alone supplies no certificate.
6. Both accessors return the very same RawRow, including childSlot; there is no
   receive/frame filtering, callback reordering, dynamic-state or ABI change.
7. This is narrow immutable-accessor equivalence, not a full Rust/QUIC proof.
-/

#print axioms certified_direct_equals_original
#print axioms guarded_hybrid_equals_original
#print axioms source_prefix_no_u32_overflow
#print axioms query_guards_preserve_every_outcome
end Hibana.ArmRowIndex
