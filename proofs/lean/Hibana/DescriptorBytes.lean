import Std

namespace Hibana

def readByte? (bytes : List Nat) (offset : Nat) : Option Nat :=
  match bytes[offset]? with
  | some byte => if byte < 256 then some byte else none
  | none => none

def readU16LE? (bytes : List Nat) (offset : Nat) : Option Nat := do
  let low ← readByte? bytes offset
  let high ← readByte? bytes (offset + 1)
  pure (low + high * 256)

def readU32LE? (bytes : List Nat) (offset : Nat) : Option Nat := do
  let low ← readU16LE? bytes offset
  let high ← readU16LE? bytes (offset + 2)
  pure (low + high * 65536)

def readByteRange?
    (bytes : List Nat) (offset start finish : Nat) : Option (List Nat) := do
  if start ≤ finish then pure () else none
  (List.range (finish - start)).mapM fun index =>
    readByte? bytes (offset + start + index)

end Hibana
