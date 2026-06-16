use hibana::runtime::wire::{CodecError, WireEncode, WirePayload};
use hibana::{Endpoint, g};

struct TestPayload(u8);

impl WireEncode for TestPayload {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = self.0;
        Ok(1)
    }
}

impl WirePayload for TestPayload {
    type Decoded<'a> = Self;

    fn validate_payload(input: hibana::runtime::wire::Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 1 {
            Ok(())
        } else if input.as_bytes().is_empty() {
            Err(CodecError::Truncated)
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload<'a>(input: hibana::runtime::wire::Payload<'a>) -> Self {
        let input = input.as_bytes();
        Self(input[0])
    }
}

fn pending_send_keeps_endpoint_borrow<'r>(endpoint: &mut Endpoint<'r, 0>) {
    let payload = TestPayload(1);
    let send = endpoint.send::<g::Msg<7, TestPayload>>(&payload);
    let next_payload = TestPayload(2);
    let send_again = endpoint.send::<g::Msg<7, TestPayload>>(&next_payload);
    core::hint::black_box(&send_again);
    core::hint::black_box(send);
}

fn main() {}
