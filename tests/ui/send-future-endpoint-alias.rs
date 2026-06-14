use hibana::runtime::wire::{CodecError, WireEncode, WirePayload};
use hibana::{Endpoint, g};

struct TestPayload(u8);

impl WireEncode for TestPayload {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

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

    fn zero_payload<'a>(
        scratch: &'a mut [u8],
    ) -> Result<hibana::runtime::wire::Payload<'a>, CodecError> {
        if scratch.is_empty() {
            return Err(CodecError::Truncated);
        }
        scratch[0] = 0;
        Ok(hibana::runtime::wire::Payload::new(&scratch[..1]))
    }
}

fn pending_send_keeps_endpoint_borrow<'r>(endpoint: &mut Endpoint<'r, 0>) {
    let flow = endpoint
        .flow::<g::Msg<7, TestPayload>>()
        .expect("test setup");
    let send = flow.send(&TestPayload(1));
    let flow_again = endpoint
        .flow::<g::Msg<7, TestPayload>>()
        .expect("test setup");
    core::hint::black_box(&flow_again);
    core::hint::black_box(send);
}

fn main() {}
