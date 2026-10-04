import Std

namespace Hibana.RouteColumnAccess

-- A two-byte read at a binary-search probe stays inside the checked column.
-- Arithmetic is over Nat; Rust's separate compact-domain bound excludes overflow.
theorem probe_inside_blob (offset count probe blob : Nat)
    (member : probe < count) (bound : offset + 2 * count ≤ blob) :
    offset + 2 * probe < blob ∧ offset + 2 * probe + 1 < blob := by
  omega

-- All compact u16 offsets and counts fit a 32-bit (or wider) usize calculation.
theorem compact_arithmetic_fits (offset count : Nat)
    (o : offset ≤ 65535) (c : count ≤ 65535) :
    offset + 2 * count < 4294967296 := by
  omega

end Hibana.RouteColumnAccess
