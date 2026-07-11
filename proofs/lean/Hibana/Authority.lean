import Hibana.Syntax

namespace Hibana

/-- The compact byte representation used at Rust route-authority boundaries. -/
def decodeRouteArm? : Nat -> Option RouteArm
  | 0 => some .left
  | 1 => some .right
  | _ + 2 => none

def encodeRouteArm : RouteArm -> Nat
  | .left => 0
  | .right => 1

theorem route_arm_decode_encode_round_trip (arm : RouteArm) :
    decodeRouteArm? (encodeRouteArm arm) = some arm := by
  cases arm <;> rfl

theorem route_arm_decode_accepts_only_binary
    {raw : Nat} {arm : RouteArm}
    (decoded : decodeRouteArm? raw = some arm) :
    raw = 0 \/ raw = 1 := by
  cases raw with
  | zero => exact Or.inl rfl
  | succ raw =>
      cases raw with
      | zero => exact Or.inr rfl
      | succ raw => simp [decodeRouteArm?] at decoded

theorem invalid_route_arm_decode_rejected
    {raw : Nat}
    (invalid : 2 <= raw) :
    decodeRouteArm? raw = none := by
  cases raw with
  | zero => simp at invalid
  | succ raw =>
      cases raw with
      | zero => simp at invalid
      | succ raw => rfl

/-- Descriptor nodes reserve byte 255 for absence; every other non-binary byte
is invalid rather than another spelling of absence. -/
def decodeOptionalRouteArm? (raw : Nat) : Option (Option RouteArm) :=
  if raw = 255 then some none else Option.map some (decodeRouteArm? raw)

theorem optional_route_arm_decode_encode_round_trip (arm : RouteArm) :
    decodeOptionalRouteArm? (encodeRouteArm arm) = some (some arm) := by
  cases arm <;> rfl

theorem optional_route_arm_absence_is_exact
    {raw : Nat}
    (decoded : decodeOptionalRouteArm? raw = some none) :
    raw = 255 := by
  by_cases sentinel : raw = 255
  · exact sentinel
  · simp [decodeOptionalRouteArm?, sentinel] at decoded

theorem invalid_optional_route_arm_rejected
    {raw : Nat}
    (lower : 2 <= raw)
    (upper : raw < 255) :
    decodeOptionalRouteArm? raw = none := by
  simp [decodeOptionalRouteArm?, Nat.ne_of_lt upper,
    invalid_route_arm_decode_rejected lower]

theorem selected_optional_route_arm_is_binary
    {raw : Nat} {arm : RouteArm}
    (decoded : decodeOptionalRouteArm? raw = some (some arm)) :
    raw = 0 \/ raw = 1 := by
  by_cases sentinel : raw = 255
  · subst raw
    simp [decodeOptionalRouteArm?] at decoded
  · simp [decodeOptionalRouteArm?, sentinel] at decoded
    exact route_arm_decode_accepts_only_binary decoded

structure RouteAuthorityPublication where
  selected : Option RouteArm
  deriving DecidableEq

/-- Validation precedes publication; an invalid compact arm has no successor. -/
def publishRawRouteArm?
    (state : RouteAuthorityPublication)
    (raw : Nat) : Option RouteAuthorityPublication :=
  match decodeRouteArm? raw with
  | none => none
  | some arm => some { state with selected := some arm }

theorem invalid_route_arm_has_no_publication
    {state : RouteAuthorityPublication} {raw : Nat}
    (invalid : 2 <= raw) :
    publishRawRouteArm? state raw = none := by
  simp [publishRawRouteArm?, invalid_route_arm_decode_rejected invalid]

theorem valid_route_arm_publication_is_exact
    {state : RouteAuthorityPublication} {raw : Nat} {arm : RouteArm}
    (decoded : decodeRouteArm? raw = some arm) :
    publishRawRouteArm? state raw = some { state with selected := some arm } := by
  simp [publishRawRouteArm?, decoded]

/-- The outer option validates the two-bit encoding; the inner option records
whether exactly one arm is ready. -/
def decodeSingleReadyArmMask? : Nat -> Option (Option RouteArm)
  | 0 => some none
  | 1 => some (some .left)
  | 2 => some (some .right)
  | 3 => some none
  | _ + 4 => none

theorem invalid_ready_arm_mask_rejected
    {mask : Nat}
    (invalid : 4 <= mask) :
    decodeSingleReadyArmMask? mask = none := by
  cases mask with
  | zero => simp at invalid
  | succ mask =>
      cases mask with
      | zero => simp at invalid
      | succ mask =>
          cases mask with
          | zero => simp at invalid
          | succ mask =>
              cases mask with
              | zero => simp at invalid
              | succ mask => rfl

theorem valid_ready_arm_mask_is_accepted
    {mask : Nat}
    (valid : mask <= 3) :
    (decodeSingleReadyArmMask? mask).isSome = true := by
  cases mask with
  | zero => rfl
  | succ mask =>
      cases mask with
      | zero => rfl
      | succ mask =>
          cases mask with
          | zero => rfl
          | succ mask =>
              cases mask with
              | zero => rfl
              | succ mask => simp at valid

theorem selected_ready_arm_mask_is_exact
    {mask : Nat} {arm : RouteArm}
    (decoded : decodeSingleReadyArmMask? mask = some (some arm)) :
    (mask = 1 /\ arm = .left) \/ (mask = 2 /\ arm = .right) := by
  cases mask with
  | zero => simp [decodeSingleReadyArmMask?] at decoded
  | succ mask =>
      cases mask with
      | zero =>
          simp [decodeSingleReadyArmMask?] at decoded
          exact Or.inl ⟨rfl, decoded.symm⟩
      | succ mask =>
          cases mask with
          | zero =>
              simp [decodeSingleReadyArmMask?] at decoded
              exact Or.inr ⟨rfl, decoded.symm⟩
          | succ mask =>
              cases mask with
              | zero => simp [decodeSingleReadyArmMask?] at decoded
              | succ mask => simp [decodeSingleReadyArmMask?] at decoded

end Hibana
