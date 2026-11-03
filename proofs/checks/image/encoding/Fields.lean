import Std

/-! Fixed-width fields used by wire headers, owned envelopes, route rows and
compiler bitmaps. These are representation laws, not protocol progress laws. -/
namespace Hibana.Fields

theorem append_exact (high high' : BitVec m) (low low' : BitVec n) :
    high ++ low = high' ++ low' ↔ high = high' ∧ low = low' := by
  constructor
  · intro equal
    exact ⟨by simpa only [BitVec.extractLsb'_append_eq_left] using
      congrArg (fun v => v.extractLsb' n m) equal,
      by simpa only [BitVec.extractLsb'_append_eq_right] using
        congrArg (fun v => v.extractLsb' 0 n) equal⟩
  · rintro ⟨rfl, rfl⟩
    rfl

theorem append_parts (high : BitVec m) (low : BitVec n) :
    (high ++ low).extractLsb' n m = high ∧
      (high ++ low).extractLsb' 0 n = low :=
  ⟨BitVec.extractLsb'_append_eq_left, BitVec.extractLsb'_append_eq_right⟩

def header (session : BitVec 32) (lane source target : BitVec 8)
    (event : BitVec 16) : BitVec 72 := session ++ lane ++ source ++ target ++ event

theorem header_exact (session session' : BitVec 32)
    (lane lane' source source' target target' : BitVec 8) (event event' : BitVec 16) :
    header session lane source target event = header session' lane' source' target' event' ↔
      session = session' ∧ lane = lane' ∧ source = source' ∧ target = target' ∧ event = event' := by
  unfold header
  rw [append_exact (session ++ lane ++ source ++ target)
    (session' ++ lane' ++ source' ++ target') event event']
  rw [append_exact (session ++ lane ++ source)
    (session' ++ lane' ++ source') target target']
  rw [append_exact (session ++ lane) (session' ++ lane') source source']
  rw [append_exact session session' lane lane']
  simp only [and_assoc]

theorem header_event (session : BitVec 32) (lane source target : BitVec 8)
    (event : BitVec 16) :
    (header session lane source target event).extractLsb' 0 16 = event :=
  BitVec.extractLsb'_append_eq_right

theorem owned_envelope_exact (role role' : BitVec 8) (event event' : BitVec 16) :
    role ++ event = role' ++ event' ↔ role = role' ∧ event = event' :=
  append_exact role role' event event'

theorem route_row_exact (stop child : BitVec 16) :
    (stop ++ child).extractLsb' 16 16 = stop ∧
      (stop ++ child).extractLsb' 0 16 = child := append_parts stop child

theorem truncating_event_aliases :
    (256#16).extractLsb' 0 8 = (0#16).extractLsb' 0 8 ∧ 256#16 ≠ 0#16 := by decide

theorem full_owned_boundary : (255#8 ++ 65535#16) = 16777215#24 := by decide

theorem appended_intersection_empty (a a' : BitVec m) (b b' : BitVec n) :
    ((a ++ b) &&& (a' ++ b')) = 0#(m + n) ↔ a &&& a' = 0#m ∧ b &&& b' = 0#n := by
  rw [BitVec.and_append, ← BitVec.zero_append_zero, append_exact]

theorem four_word_intersection_empty
    (a b c d a' b' c' d' : BitVec 64) :
    ((a ++ b ++ c ++ d) &&& (a' ++ b' ++ c' ++ d')) = 0#256 ↔
      a &&& a' = 0#64 ∧ b &&& b' = 0#64 ∧ c &&& c' = 0#64 ∧ d &&& d' = 0#64 := by
  rw [appended_intersection_empty (a ++ b ++ c) (a' ++ b' ++ c') d d',
    appended_intersection_empty (a ++ b) (a' ++ b') c c',
    appended_intersection_empty a a' b b']
  simp only [and_assoc]

theorem role_bitmap_exact (first second target : Fin 256) (owned owned' : Bool) :
    (((if owned then BitVec.twoPow 256 first.val else 0) |||
      (if owned' then BitVec.twoPow 256 second.val else 0))).getLsbD target.val =
      ((owned && decide (first = target)) || (owned' && decide (second = target))) := by
  cases owned <;> cases owned' <;>
    simp [Fin.ext_iff, eq_comm]

theorem row_address_exact (width row row' column column' : Nat)
    (positive : 0 < width) (within : column < width) (within' : column' < width) :
    width * row + column = width * row' + column' ↔ row = row' ∧ column = column' := by
  constructor
  · intro equal
    have rows := congrArg (fun v => v / width) equal
    simp only [Nat.mul_add_div positive, Nat.div_eq_of_lt within,
      Nat.div_eq_of_lt within', Nat.add_zero] at rows
    subst row'
    exact ⟨rfl, by omega⟩
  · rintro ⟨rfl, rfl⟩
    rfl

set_option maxRecDepth 2048 in
theorem ceil_row_width (colors : Nat) (positive : 0 < colors) (bound : colors ≤ 256) :
    0 < (colors + 7) / 8 * 8 ∧ colors ≤ (colors + 7) / 8 * 8 ∧
      (colors + 7) / 8 * 8 ≤ 256 := by
  have all : ∀ colors : Fin 257, 0 < colors.val →
      0 < (colors.val + 7) / 8 * 8 ∧ colors.val ≤ (colors.val + 7) / 8 * 8 ∧
        (colors.val + 7) / 8 * 8 ≤ 256 := by decide
  exact all ⟨colors, by omega⟩ positive

theorem compact_table_bound (row column colors : Nat)
    (r : row < 256) (c : column < colors) (positive : 0 < colors) (bound : colors ≤ 256) :
    ((colors + 7) / 8 * 8) * row + column < 65536 := by
  have widths := ceil_row_width colors positive bound
  generalize (colors + 7) / 8 * 8 = width at widths ⊢
  have product := Nat.mul_le_mul_right row widths.2.2
  omega

theorem truncated_row_width_aliases :
    (0 : Nat) ≠ 1 ∧ 0 * (1 / 8 * 8) + 0 = 1 * (1 / 8 * 8) + 0 := by decide

end Hibana.Fields

#print axioms Hibana.Fields.append_exact
#print axioms Hibana.Fields.append_parts
#print axioms Hibana.Fields.header_exact
#print axioms Hibana.Fields.header_event
#print axioms Hibana.Fields.owned_envelope_exact
#print axioms Hibana.Fields.route_row_exact
#print axioms Hibana.Fields.truncating_event_aliases
#print axioms Hibana.Fields.full_owned_boundary
#print axioms Hibana.Fields.appended_intersection_empty
#print axioms Hibana.Fields.four_word_intersection_empty
#print axioms Hibana.Fields.role_bitmap_exact
#print axioms Hibana.Fields.row_address_exact
#print axioms Hibana.Fields.ceil_row_width
#print axioms Hibana.Fields.compact_table_bound
#print axioms Hibana.Fields.truncated_row_width_aliases
