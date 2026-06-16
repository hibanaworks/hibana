use hibana::{
    Endpoint, g,
    runtime::wire::{CodecError, WireEncode},
};

struct SendOnly(u8);

impl WireEncode for SendOnly {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = self.0;
        Ok(1)
    }
}

fn send_only(endpoint: &mut Endpoint<'_, 0>) {
    let payload = SendOnly(7);
    let Ok(flow) = endpoint.flow::<g::Msg<33, SendOnly>>() else {
        return;
    };
    let future = flow.send(&payload);
    core::mem::drop(future);
}

fn main() {}
