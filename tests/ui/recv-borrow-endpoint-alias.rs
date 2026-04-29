use hibana::substrate::{
    SessionKit, Transport,
    runtime::{Clock, LabelUniverse},
    wire::{CodecError, Payload, WireEncode, WirePayload},
};
use hibana::{Endpoint, g};

struct FramePayload([u8; 4]);

impl WireEncode for FramePayload {
    fn encoded_len(&self) -> Option<usize> {
        Some(self.0.len())
    }

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

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Ok(input)
    }
}

fn borrowed_recv_keeps_endpoint_borrow<'r, 'cfg, T, U, C, const MAX_RV: usize>(
    endpoint: &mut Endpoint<'r, 0, SessionKit<'cfg, T, U, C, MAX_RV>>,
) where
    T: Transport + 'cfg,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
    'cfg: 'r,
{
    let payload = futures::executor::block_on(endpoint.recv::<g::Msg<7, FramePayload>>()).unwrap();
    let flow_again = endpoint.flow::<g::Msg<8, u8>>().unwrap();
    core::hint::black_box(&flow_again);
    core::hint::black_box(&payload);
}

fn main() {}
