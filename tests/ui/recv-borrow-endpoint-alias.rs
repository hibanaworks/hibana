use hibana::runtime::wire::{CodecError, Payload, WireEncode, WirePayload};
use hibana::{Endpoint, g};

struct FramePayload([u8; 4]);

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

fn borrowed_recv_keeps_endpoint_borrow<'r>(endpoint: &mut Endpoint<'r, 0>) {
    let payload = futures::executor::block_on(endpoint.recv::<g::Msg<7, FramePayload>>())
        .expect("test setup");
    let flow_again = endpoint.flow::<g::Msg<8, u8>>().expect("test setup");
    core::hint::black_box(&flow_again);
    core::hint::black_box(&payload);
}

fn main() {}
