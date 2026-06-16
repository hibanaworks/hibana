use hibana::{
    Endpoint, RouteBranch, g,
    runtime::wire::{CodecError, Payload, WirePayload},
};

struct RecvOnly;

impl WirePayload for RecvOnly {
    type Decoded<'a> = Payload<'a>;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().is_empty() {
            Ok(())
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input
    }
}

fn recv_only(endpoint: &mut Endpoint<'_, 0>) {
    let future = endpoint.recv::<g::Msg<34, RecvOnly>>();
    core::mem::drop(future);
}

fn decode_only(branch: RouteBranch<'_, '_, 0>) {
    let future = branch.decode::<g::Msg<34, RecvOnly>>();
    core::mem::drop(future);
}

fn main() {}
