use hibana::{
    Endpoint, RouteBranch, g,
    runtime::wire::{CodecError, Payload, WirePayload},
};

struct RecvOnly;

impl WirePayload for RecvOnly {
    type Decoded<'a> = ();

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().is_empty() {
            Ok(())
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        input.as_bytes();
    }
}

fn send_recv_only(endpoint: &mut Endpoint<'_, 0>) {
    let payload = RecvOnly;
    let future = endpoint.send::<g::Msg<36, RecvOnly>>(&payload);
    core::mem::drop(future);
}

fn branch_send_recv_only(branch: RouteBranch<'_, '_, 0>) {
    let payload = RecvOnly;
    let future = branch.send::<g::Msg<36, RecvOnly>>(&payload);
    core::mem::drop(future);
}

fn main() {}
