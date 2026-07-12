use hibana::{g, runtime};

fn main() {
    let duplex_stream = g::par(
        g::send::<0, 1, g::Msg<100, [u8; 16]>>(),
        g::send::<1, 0, g::Msg<101, [u8; 16]>>(),
    )
    .roll();
    let stream_side_0: runtime::program::RoleProgram<0> =
        runtime::program::project(&duplex_stream);
    let stream_side_1: runtime::program::RoleProgram<1> =
        runtime::program::project(&duplex_stream);

    let repeated_rpc = g::seq(
        g::send::<0, 1, g::Msg<110, [u8; 8]>>(),
        g::send::<1, 0, g::Msg<111, bool>>(),
    )
    .roll();
    let rpc_caller: runtime::program::RoleProgram<0> =
        runtime::program::project(&repeated_rpc);
    let rpc_responder: runtime::program::RoleProgram<1> =
        runtime::program::project(&repeated_rpc);

    core::hint::black_box((stream_side_0, stream_side_1, rpc_caller, rpc_responder));
}
