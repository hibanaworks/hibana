//! Transport codec helpers.
//!
//! `WireEncode` is the send-side contract. `WirePayload` is the receive-side
//! contract. `Payload` is the borrowed byte view passed from transport into a
//! decoder.
//!
//! Decoding is exact for built-in fixed-size payloads: trailing bytes are
//! rejected. Borrowed payload types may return views tied to the endpoint borrow.
//! Protocol-specific transports map descriptor decisions onto their native frame
//! formats; Hibana metadata is not placed on the application wire by this layer.

use core::fmt;

/// Errors surfaced by wire encode/decode helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    Truncated,
    Malformed,
}

#[inline]
pub(crate) fn require_exact_len(actual: usize, expected: usize) -> Result<(), CodecError> {
    if actual < expected {
        Err(CodecError::Truncated)
    } else if actual == expected {
        Ok(())
    } else {
        Err(CodecError::Malformed)
    }
}

/// Send-side payload encoding contract.
pub trait WireEncode {
    /// Encode into `out`, returning the number of bytes written.
    ///
    /// An unchanged value must produce the same bytes on repeated calls. Hibana
    /// may encode again while progressing a parked transport without retaining a
    /// borrow into shared rendezvous scratch between polls.
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError>;
}

#[inline(always)]
pub(crate) const fn erased_encoder<P: WireEncode>()
-> unsafe fn(*const (), &mut [u8]) -> Result<usize, CodecError> {
    encode_erased::<P>
}

#[inline(always)]
unsafe fn encode_erased<P: WireEncode>(
    ptr: *const (),
    scratch: &mut [u8],
) -> Result<usize, CodecError> {
    // SAFETY: callers pair an erased pointer produced from `&P` with this
    // exact encoder. The future or kernel owner keeps the source borrow live.
    let payload = unsafe { &*ptr.cast::<P>() };
    payload.encode_into(scratch)
}

/// Receive-side payload decoding contract.
///
/// `Decoded<'a>` describes what receive operations yield when wire bytes are
/// borrowed for the duration of the endpoint borrow.
pub trait WirePayload {
    /// Protocol-local identity of this payload's wire schema.
    ///
    /// This identifies the canonical encoding and validation contract, not a
    /// Rust nominal type. Incompatible wire contracts must use distinct
    /// identities. Two binaries may use different Rust wrapper types only when
    /// they intentionally implement the same canonical wire schema. Identity
    /// `0` is the canonical zero-byte unit schema used by `()`.
    /// Another Rust wrapper may use it only if its encoder and validator also
    /// accept exactly zero bytes. Local actions verify that invariant before
    /// committing progress. Other custom schemas use nonzero identities.
    /// The value is descriptor metadata and is not transmitted on the wire.
    const SCHEMA_ID: u32;

    type Decoded<'a>;

    /// Validate payload-local bytes before endpoint progress can commit.
    ///
    /// Checks that require choreography descriptor context, endpoint role, or
    /// session/lane identity are owned by the endpoint kernel. Those contextual
    /// checks run after payload-local validation and
    /// before receive progress commits.
    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError>;

    /// Decode bytes already accepted by payload-local validation and any
    /// endpoint-context validation owned by the calling kernel path.
    ///
    /// Endpoint receive/decode progress is committed before this decode function runs,
    /// so this operation has no error channel.
    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a>;
}

impl WireEncode for () {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(out[..0].len())
    }
}

impl WirePayload for () {
    const SCHEMA_ID: u32 = 0;

    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        require_exact_len(input.as_bytes().len(), 0)
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        if !input.as_bytes().is_empty() {
            crate::invariant();
        }
    }
}

impl WireEncode for bool {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = if *self { 1 } else { 0 };
        Ok(1)
    }
}

impl WirePayload for bool {
    const SCHEMA_ID: u32 = 1;

    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        let bytes = input.as_bytes();
        require_exact_len(bytes.len(), 1)?;
        match bytes[0] {
            0 | 1 => Ok(()),
            _ => Err(CodecError::Malformed),
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input.as_bytes()[0] != 0
    }
}

macro_rules! impl_wire_for_int {
    ($ty:ty, $len:expr, $schema:expr) => {
        impl WireEncode for $ty {
            fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
                if out.len() < $len {
                    return Err(CodecError::Truncated);
                }
                out[..$len].copy_from_slice(&self.to_be_bytes());
                Ok($len)
            }
        }

        impl WirePayload for $ty {
            const SCHEMA_ID: u32 = $schema;

            type Decoded<'a> = Self;

            fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
                require_exact_len(input.as_bytes().len(), $len)
            }

            fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
                let bytes = input.as_bytes();
                let mut buf = [0u8; $len];
                buf.copy_from_slice(&bytes[..$len]);
                <$ty>::from_be_bytes(buf)
            }
        }
    };
}

impl_wire_for_int!(u8, 1, 2);
impl_wire_for_int!(i8, 1, 3);
impl_wire_for_int!(u16, 2, 4);
impl_wire_for_int!(i16, 2, 5);
impl_wire_for_int!(u32, 4, 6);
impl_wire_for_int!(i32, 4, 7);
impl_wire_for_int!(u64, 8, 8);
impl_wire_for_int!(i64, 8, 9);
impl_wire_for_int!(u128, 16, 10);
impl_wire_for_int!(i128, 16, 11);

impl WireEncode for &[u8] {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < self.len() {
            return Err(CodecError::Truncated);
        }
        out[..self.len()].copy_from_slice(self);
        Ok(self.len())
    }
}

impl WirePayload for &[u8] {
    const SCHEMA_ID: u32 = 12;

    type Decoded<'a> = &'a [u8];

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        input.as_bytes();
        Ok(())
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input.as_bytes()
    }
}

impl<const N: usize> WireEncode for [u8; N] {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < N {
            return Err(CodecError::Truncated);
        }
        out[..N].copy_from_slice(self);
        Ok(N)
    }
}

impl<const N: usize> WirePayload for [u8; N] {
    const SCHEMA_ID: u32 = {
        if N > 0x00ff_ffff {
            crate::invariant();
        }
        0x0100_0000 | N as u32
    };

    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        require_exact_len(input.as_bytes().len(), N)
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let bytes = input.as_bytes();
        let mut buf = [0u8; N];
        buf.copy_from_slice(&bytes[..N]);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode<'a, P: WirePayload>(input: &'a [u8]) -> Result<P::Decoded<'a>, CodecError> {
        let payload = Payload::new(input);
        P::validate_payload(payload)?;
        Ok(P::decode_validated_payload(payload))
    }

    #[test]
    fn fixed_payload_decoders_reject_trailing_bytes() {
        assert_eq!(decode::<()>(&[]), Ok(()));
        assert_eq!(decode::<()>(&[1]), Err(CodecError::Malformed));

        assert_eq!(decode::<bool>(&[1]), Ok(true));
        assert_eq!(decode::<bool>(&[1, 0]), Err(CodecError::Malformed));

        assert_eq!(decode::<u16>(&[0x12, 0x34]), Ok(0x1234));
        assert_eq!(
            decode::<u16>(&[0x12, 0x34, 0x56]),
            Err(CodecError::Malformed)
        );

        assert_eq!(decode::<[u8; 2]>(&[7, 9]), Ok([7, 9]));
        assert_eq!(decode::<[u8; 2]>(&[7, 9, 11]), Err(CodecError::Malformed));
    }

    #[test]
    fn borrowed_byte_slice_remains_variable_length() {
        let bytes = [1, 2, 3];
        assert_eq!(decode::<&[u8]>(&bytes), Ok(&bytes[..]));
    }

    #[test]
    fn builtin_payload_schemas_are_pairwise_distinct() {
        let schemas = [
            <() as WirePayload>::SCHEMA_ID,
            <bool as WirePayload>::SCHEMA_ID,
            <u8 as WirePayload>::SCHEMA_ID,
            <i8 as WirePayload>::SCHEMA_ID,
            <u16 as WirePayload>::SCHEMA_ID,
            <i16 as WirePayload>::SCHEMA_ID,
            <u32 as WirePayload>::SCHEMA_ID,
            <i32 as WirePayload>::SCHEMA_ID,
            <u64 as WirePayload>::SCHEMA_ID,
            <i64 as WirePayload>::SCHEMA_ID,
            <u128 as WirePayload>::SCHEMA_ID,
            <i128 as WirePayload>::SCHEMA_ID,
            <&[u8] as WirePayload>::SCHEMA_ID,
            <[u8; 0] as WirePayload>::SCHEMA_ID,
            <[u8; 4] as WirePayload>::SCHEMA_ID,
        ];
        for (index, schema) in schemas.iter().enumerate() {
            assert!(!schemas[..index].contains(schema));
        }
        assert_eq!(<[u8; 0] as WirePayload>::SCHEMA_ID, 0x0100_0000);
        assert_eq!(<[u8; 4] as WirePayload>::SCHEMA_ID, 0x0100_0004);
        assert_eq!(<[u8; 0x00ff_ffff] as WirePayload>::SCHEMA_ID, 0x01ff_ffff);
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
        let preview_len = if bytes.len() > 32 { 32 } else { bytes.len() };
        f.debug_struct("Payload")
            .field("len", &bytes.len())
            .field("preview", &&bytes[..preview_len])
            .finish()
    }
}
