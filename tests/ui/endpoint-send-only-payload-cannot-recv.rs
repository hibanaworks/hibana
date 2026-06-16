use hibana::{
    Endpoint, RouteBranch, g,
    runtime::wire::{CodecError, WireEncode},
};

struct SendOnly;

impl WireEncode for SendOnly {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(out[..0].len())
    }
}

fn recv_send_only(endpoint: &mut Endpoint<'_, 0>) {
    let future = endpoint.recv::<g::Msg<35, SendOnly>>();
    core::mem::drop(future);
}

fn branch_recv_send_only(branch: RouteBranch<'_, '_, 0>) {
    let future = branch.recv::<g::Msg<35, SendOnly>>();
    core::mem::drop(future);
}

fn main() {}
