use hibana::runtime::wire::{CodecError, WireEncode, WirePayload};
use hibana::{Endpoint, g};

struct Payload(u8);

impl WireEncode for Payload {
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

impl WirePayload for Payload {
    type Decoded<'a> = Self;

    fn validate_payload(input: hibana::runtime::wire::Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 1 {
            Ok(())
        } else if input.as_bytes().is_empty() {
            Err(CodecError::Truncated)
        } else {
            Err(CodecError::Invalid("trailing bytes after Payload"))
        }
    }

    fn decode_validated_payload<'a>(input: hibana::runtime::wire::Payload<'a>) -> Self {
        let input = input.as_bytes();
        Self(input[0])
    }
}

fn pending_send_keeps_endpoint_borrow<'r>(endpoint: &mut Endpoint<'r, 0>) {
    let flow = endpoint.flow::<g::Msg<7, Payload>>().expect("fixture setup");
    let send = flow.send(&Payload(1));
    let flow_again = endpoint.flow::<g::Msg<7, Payload>>().expect("fixture setup");
    core::hint::black_box(&flow_again);
    core::hint::black_box(send);
}

fn main() {}
