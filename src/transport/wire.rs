//! Transport codec helpers.
//!
//! This module intentionally stays tiny: it provides the fixed codec traits
//! (`WireEncode` / `WireDecode`) used by control / mgmt payloads plus the
//! transport-facing payload wrapper (`Payload`) and lightweight codec traits
//! (`CodecError`, `WireEncode`, `WireDecode`). Protocol-specific transports
//! (QUIC, etc.) map typestate decisions onto their native frame formats and do
//! **not** put Hibana metadata on the wire. Application payloads are opaque
//! byte slices; only control-plane messages implement the lightweight codecs
//! defined here so that Hibana can stay `no_std` / `no_alloc`.

use core::{fmt, ops};

/// Errors surfaced by wire encode/decode helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    Truncated,
    Invalid(&'static str),
}

/// Trait for encoding structured payloads into transport-provided buffers.
pub trait WireEncode {
    /// Optional hint describing the encoded length if it is statically known.
    fn encoded_len(&self) -> Option<usize> {
        None
    }

    /// Encode into `out`, returning the number of bytes written.
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError>;
}

/// Trait for decoding borrowed payload slices without allocations.
pub trait WireDecode<'a>: Sized {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError>;
}

/// Helper trait for payloads that never borrow from the wire buffer (used by
/// control/mgmt messages). Application payloads are free to be raw byte slices;
/// only Hibana-defined control planes implement this trait so that codec logic
/// stays allocation-free.
pub trait WireDecodeOwned: Sized {
    fn decode_owned(input: &[u8]) -> Result<Self, CodecError>;
}

impl<T> WireDecodeOwned for T
where
    for<'a> T: WireDecode<'a>,
{
    #[inline]
    fn decode_owned(input: &[u8]) -> Result<Self, CodecError> {
        <Self as WireDecode>::decode_from(input)
    }
}

impl WireEncode for () {
    fn encoded_len(&self) -> Option<usize> {
        Some(0)
    }

    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(0)
    }
}

impl<'a> WireDecode<'a> for () {
    fn decode_from(_input: &'a [u8]) -> Result<Self, CodecError> {
        Ok(())
    }
}

impl WireEncode for bool {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = if *self { 1 } else { 0 };
        Ok(1)
    }
}

impl<'a> WireDecode<'a> for bool {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        match input[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Invalid("boolean must be 0 or 1")),
        }
    }
}

macro_rules! impl_wire_for_int {
    ($ty:ty, $len:expr) => {
        impl WireEncode for $ty {
            fn encoded_len(&self) -> Option<usize> {
                Some($len)
            }

            fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
                if out.len() < $len {
                    return Err(CodecError::Truncated);
                }
                out[..$len].copy_from_slice(&self.to_be_bytes());
                Ok($len)
            }
        }

        impl<'a> WireDecode<'a> for $ty {
            fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
                if input.len() < $len {
                    return Err(CodecError::Truncated);
                }
                let mut buf = [0u8; $len];
                buf.copy_from_slice(&input[..$len]);
                Ok(<$ty>::from_be_bytes(buf))
            }
        }
    };
}

impl_wire_for_int!(u8, 1);
impl_wire_for_int!(i8, 1);
impl_wire_for_int!(u16, 2);
impl_wire_for_int!(i16, 2);
impl_wire_for_int!(u32, 4);
impl_wire_for_int!(i32, 4);
impl_wire_for_int!(u64, 8);
impl_wire_for_int!(i64, 8);
impl_wire_for_int!(u128, 16);
impl_wire_for_int!(i128, 16);

impl WireEncode for &[u8] {
    fn encoded_len(&self) -> Option<usize> {
        Some(self.len())
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < self.len() {
            return Err(CodecError::Truncated);
        }
        out[..self.len()].copy_from_slice(self);
        Ok(self.len())
    }
}

impl<'a> WireDecode<'a> for &'a [u8] {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        Ok(input)
    }
}

impl<const N: usize> WireEncode for [u8; N] {
    fn encoded_len(&self) -> Option<usize> {
        Some(N)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < N {
            return Err(CodecError::Truncated);
        }
        out[..N].copy_from_slice(self);
        Ok(N)
    }
}

impl<'a, const N: usize> WireDecode<'a> for [u8; N] {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < N {
            return Err(CodecError::Truncated);
        }
        let mut buf = [0u8; N];
        buf.copy_from_slice(&input[..N]);
        Ok(buf)
    }
}

/// Wire-level flags for frames (no external crates).
///
/// Only transport fragmentation metadata remains here; control-plane signalling
/// moved to typed labels (`control::cap::payload`) so that the data plane observes a
/// uniform message model.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Default)]
#[repr(transparent)]
pub struct FrameFlags(u8);

impl FrameFlags {
    /// Mask of supported bits. Unknown bits are dropped when decoding.
    pub const ALLOWED: u8 = 0x10 /*FRAG*/ | 0x20 /*IDX*/ | 0x40 /*TOT*/;

    pub const EMPTY: Self = Self(0x00);
    pub const FRAG: Self = Self(0x10);
    pub const IDX: Self = Self(0x20);
    pub const TOT: Self = Self(0x40);

    #[inline]
    pub const fn empty() -> Self {
        Self::EMPTY
    }

    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Drop any unknown bits when converting from the wire representation.
    #[inline]
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::ALLOWED)
    }

    /// Accept the bit-pattern only if it does not contain unknown flag bits.
    #[inline]
    pub const fn from_bits_retain(bits: u8) -> Option<Self> {
        if (bits & !Self::ALLOWED) == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True if *all* bits in `other` are set in `self`.
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
        self.0 &= Self::ALLOWED;
    }

    #[inline]
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    #[inline]
    pub fn set(&mut self, other: Self, on: bool) {
        if on {
            self.insert(other);
        } else {
            self.remove(other);
        }
    }
}

// ---- bit-ops (mask within ALLOWED) ----
impl ops::BitOr for FrameFlags {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self((self.0 | rhs.0) & Self::ALLOWED)
    }
}

impl ops::BitOrAssign for FrameFlags {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 = (self.0 | rhs.0) & Self::ALLOWED;
    }
}

impl ops::BitAnd for FrameFlags {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl ops::BitAndAssign for FrameFlags {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl ops::BitXor for FrameFlags {
    type Output = Self;

    #[inline]
    fn bitxor(self, rhs: Self) -> Self::Output {
        Self((self.0 ^ rhs.0) & Self::ALLOWED)
    }
}

impl ops::BitXorAssign for FrameFlags {
    #[inline]
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0 = (self.0 ^ rhs.0) & Self::ALLOWED;
    }
}

impl ops::Not for FrameFlags {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self(Self::ALLOWED & !self.0)
    }
}

impl fmt::Debug for FrameFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        write!(f, "FrameFlags{{")?;
        macro_rules! push {
            ($flag:ident) => {
                if self.contains(FrameFlags::$flag) {
                    if !first {
                        write!(f, "|")?;
                    }
                    first = false;
                    write!(f, stringify!($flag))?;
                }
            };
        }
        push!(FRAG);
        push!(IDX);
        push!(TOT);
        if first {
            write!(f, "EMPTY")?;
        }
        write!(f, "}}")
    }
}

/// Zero-copy view over an encoded payload slice (application data remains
/// opaque to Hibana; transports simply forward the bytes handed to them).
pub struct Payload<'a> {
    data: &'a [u8],
}
impl<'a> Payload<'a> {
    #[inline]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    #[inline]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
}

impl<'a> fmt::Debug for Payload<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.as_bytes();
        let preview_len = bytes.len().min(32);
        f.debug_struct("Payload")
            .field("len", &bytes.len())
            .field("preview", &&bytes[..preview_len])
            .finish()
    }
}
