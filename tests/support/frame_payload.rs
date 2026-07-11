use hibana::runtime::wire::{CodecError, Payload, WireEncode, WirePayload};

#[derive(Clone, Copy)]
pub(crate) struct FramePayload(pub(crate) [u8; 4]);

impl WireEncode for FramePayload {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < self.0.len() {
            return Err(CodecError::Truncated);
        }
        out[..self.0.len()].copy_from_slice(&self.0);
        Ok(self.0.len())
    }
}

impl WirePayload for FramePayload {
    const SCHEMA_ID: u32 = 0x4000_0001;

    type Decoded<'a> = Payload<'a>;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        match input.as_bytes().len() {
            4 => Ok(()),
            0..=3 => Err(CodecError::Truncated),
            _ => Err(CodecError::Malformed),
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input
    }
}
