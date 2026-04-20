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

fn branch_decode_is_affine<'r, 'cfg, T, U, C, const MAX_RV: usize>(
    endpoint: &mut Endpoint<'r, 0, SessionKit<'cfg, T, U, C, MAX_RV>>,
) where
    T: Transport + 'cfg,
    U: LabelUniverse + 'cfg,
    C: Clock + 'cfg,
    'cfg: 'r,
{
    let branch = futures::executor::block_on(endpoint.offer()).unwrap();
    let _first = branch.decode::<g::Msg<7, FramePayload>>();
    let _second = branch.decode::<g::Msg<7, FramePayload>>();
}

fn main() {}
