# Capacity correspondence scope

Rust lowering aborts globally when any final color domain exhausts 256 colors.
The total Lean reference retains invalid label 256 in that global event row.
The exact role-label equality rejects a certificate only if its role filter
retains an invalid row. An unrelated role can omit the row. This work does not
prove the existing per-role checker globally rejects every arbitrary synthetic
certificate for an overflowing source, and does not add such a guard.

The positive Rust-to-reference correspondence is scoped to successfully lowered
Rust sources, whose final colors fit the byte domain. The negative admission
theorem has an explicit participation/retained-row premise, `256 ∈ labels`.
The Python/Z3 whole-list acceptance comparison additionally requires that no
invalid 256 occurs anywhere in the total reference result; this is an explicit
model acceptance assumption, not a proved existing per-role checker property.

`CapacityScope.lean` proves the non-vacuity boundary directly: an unrelated role
filters the invalid row to `[]`, while a participating role retains `[256]`.
It then proves rejection under the retained-row premise using the unchanged
byte decoder. No certificate/checker/count/axiom restriction is weakened.
