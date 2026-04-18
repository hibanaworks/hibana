//! Transport codec helpers.
//!
//! The canonical payload contract is [`WirePayload`]. [`Payload`] is the
//! transport-facing borrowed byte view passed into that contract. In-tree
//! control/mgmt payloads implement by-value [`WirePayload`] directly when
//! borrowed views are unnecessary.
//! Protocol-specific transports map typestate decisions onto their native frame
//! formats and do **not** put Hibana metadata on the wire.

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
    fn encoded_len(&self) -> Option<usize>;

    /// Encode into `out`, returning the number of bytes written.
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError>;
}

/// Payload owner contract for app-facing receive/decode paths.
///
/// `Payload` remains the send-side owner that `flow().send()` accepts by
/// reference. `Decoded<'a>` describes what `recv()` / `decode()` yield when the
/// wire bytes are borrowed for the duration of the endpoint borrow.
pub trait WirePayload: WireEncode {
    type Decoded<'a>;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError>;
}

impl WireEncode for () {
    fn encoded_len(&self) -> Option<usize> {
        Some(0)
    }

    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(0)
    }
}

impl WirePayload for () {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(_input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
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

impl WirePayload for bool {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return Err(CodecError::Truncated);
        }
        match bytes[0] {
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

        impl WirePayload for $ty {
            type Decoded<'a> = Self;

            fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
                let bytes = input.as_bytes();
                if bytes.len() < $len {
                    return Err(CodecError::Truncated);
                }
                let mut buf = [0u8; $len];
                buf.copy_from_slice(&bytes[..$len]);
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

impl WirePayload for &[u8] {
    type Decoded<'a> = &'a [u8];

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Ok(input.as_bytes())
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

impl<const N: usize> WirePayload for [u8; N] {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() < N {
            return Err(CodecError::Truncated);
        }
        let mut buf = [0u8; N];
        buf.copy_from_slice(&bytes[..N]);
        Ok(buf)
    }
}

/// Wire-level flags for frames (no external crates).
///
/// Only transport fragmentation metadata remains here; control-plane signalling
/// stays on typed control messages so that the data plane observes a uniform
/// message model.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Default)]
#[repr(transparent)]
pub(crate) struct FrameFlags(u8);

impl FrameFlags {
    /// Mask of supported bits. Unknown bits are dropped when decoding.
    pub(crate) const ALLOWED: u8 = 0x10 /*FRAG*/ | 0x20 /*IDX*/ | 0x40 /*TOT*/;

    pub(crate) const EMPTY: Self = Self(0x00);
    pub(crate) const FRAG: Self = Self(0x10);
    pub(crate) const IDX: Self = Self(0x20);
    pub(crate) const TOT: Self = Self(0x40);

    #[inline]
    pub(crate) const fn empty() -> Self {
        Self::EMPTY
    }

    #[inline]
    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    /// True if *all* bits in `other` are set in `self`.
    #[inline]
    pub(crate) const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
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
#[derive(Clone, Copy)]
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
