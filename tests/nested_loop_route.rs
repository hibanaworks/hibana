use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};
use hibana::substrate::program::{RoleProgram, project};

const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;

#[test]
fn nested_loop_scope_balanced() {
    let tick = g::send::<Role<2>, Role<3>, Msg<1, ()>, 0>();
    let ack_branch = g::seq(
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >()
        .policy::<10>(),
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >(),
    );
    let loss_branch = g::seq(
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
        .policy::<10>(),
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >(),
    );
    let ack_loss_route = g::route(ack_branch, loss_branch);
    let continue_arm = g::seq(
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ LABEL_LOOP_CONTINUE }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >()
        .policy::<11>(),
        g::seq(tick, ack_loss_route),
    );
    let break_arm = g::send::<
        Role<2>,
        Role<2>,
        Msg<{ LABEL_LOOP_BREAK }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<11>();
    let decision = g::route(continue_arm, break_arm);

    let _role_program: RoleProgram<2> = project(&decision);

    let handshake = g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>();
    let combined = g::par(handshake, decision);
    let _transport_program: RoleProgram<2> = project(&combined);
}
