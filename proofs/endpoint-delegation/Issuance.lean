/- Initial resource issuance, independent of protocol cursor progress. -/
namespace EndpointDelegation.Issuance

inductive State where
  | available
  | reserved
  | issued
  deriving DecidableEq

inductive Step : State → State → Prop where
  | reserve : Step .available .reserved
  | abandon : Step .reserved .available
  | publish : Step .reserved .issued

inductive Steps : State → State → Prop where
  | refl (state) : Steps state state
  | tail : Steps first middle → Step middle last → Steps first last

theorem issued_has_no_step (step : Step .issued next) : False := by
  cases step

theorem issued_is_permanent (path : Steps .issued last) : last = .issued := by
  induction path with
  | refl => rfl
  | tail _ step ih =>
    subst ih
    exact False.elim (issued_has_no_step step)

theorem publication_prevents_another_reservation
    (path : Steps .issued state) (step : Step state next) : False := by
  have h := issued_is_permanent path
  subst h
  exact issued_has_no_step step

theorem failed_unpublished_attach_preserves_issuance : Steps .available .available := by
  exact .tail (.tail (.refl _) .reserve) .abandon

#print axioms issued_has_no_step
#print axioms issued_is_permanent
#print axioms publication_prevents_another_reservation
#print axioms failed_unpublished_attach_preserves_issuance

end EndpointDelegation.Issuance
