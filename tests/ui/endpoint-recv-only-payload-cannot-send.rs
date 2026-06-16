use hibana::{
    Endpoint, g,
    runtime::wire::{CodecError, Payload, WirePayload},
};

struct RecvOnly;

impl WirePayload for RecvOnly {
    const ALLOWS_ZERO_LENGTH: bool = true;

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
    let Ok(flow) = endpoint.flow::<g::Msg<36, RecvOnly>>() else {
        return;
    };
    let future = flow.send(&payload);
    core::mem::drop(future);
}

fn main() {}
