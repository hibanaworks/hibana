use hibana::substrate::{
    SessionKit, Transport,
    runtime::{Clock, LabelUniverse},
    wire::{CodecError, WireEncode, WirePayload},
};
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

    fn decode_payload<'a>(input: hibana::substrate::wire::Payload<'a>) -> Result<Self, CodecError> {
        let input = input.as_bytes();
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        Ok(Self(input[0]))
    }
}

fn pending_send_keeps_endpoint_borrow<'r, 'cfg, T, U, C, const MAX_RV: usize>(
    endpoint: &mut Endpoint<'r, 0, SessionKit<'cfg, T, U, C, MAX_RV>>,
) where
    T: Transport + 'cfg,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
    'cfg: 'r,
{
    let flow = endpoint.flow::<g::Msg<7, Payload>>().unwrap();
    let send = flow.send(&Payload(1));
    let _flow_again = endpoint.flow::<g::Msg<7, Payload>>().unwrap();
    let _ = send;
}

fn main() {}
