use hibana::{g, runtime};

fn main() {
    let bidirectional_loop = g::par(
        g::send::<0, 1, g::Msg<100, [u8; 16]>>(),
        g::send::<1, 0, g::Msg<101, [u8; 16]>>(),
    )
    .roll();
    let side_0: runtime::program::RoleProgram<0> =
        runtime::program::project(&bidirectional_loop);
    let side_1: runtime::program::RoleProgram<1> =
        runtime::program::project(&bidirectional_loop);

    let request_reply_loop = g::seq(
        g::send::<0, 1, g::Msg<110, [u8; 8]>>(),
        g::send::<1, 0, g::Msg<111, bool>>(),
    )
    .roll();
    let initiator: runtime::program::RoleProgram<0> =
        runtime::program::project(&request_reply_loop);
    let responder: runtime::program::RoleProgram<1> =
        runtime::program::project(&request_reply_loop);

    core::hint::black_box((side_0, side_1, initiator, responder));
}
